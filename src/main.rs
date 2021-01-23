#![feature(try_blocks)]

use fantoccini::{Client, Locator, Element};
use std::io::Write;
use std::convert::TryInto;
use anyhow::{ Result, bail, Context};
//use tokio::process::Command;
use tokio::time::{sleep, Duration, Instant};
use tokio::time::timeout;
use fantoccini::error::CmdError;
use read_input::prelude::*;
use rpassword::*;

// let's set up the sequence of steps we want the browser to take
#[tokio::main]
async fn main() -> Result<()> {
    let mut caps = serde_json::map::Map::new();
    let opts = serde_json::json!({
        "args": ["--user-data-dir=tweetus-deletus-browser-data"]
    });
    caps.insert("goog:chromeOptions".to_string(), opts);

    //let _out = Command::new("geckodriver").output().await?;
    let mut c = Client::with_capabilities("http://localhost:9515", caps).await
        .context("failed to connect to WebDriver")?;

    let browse_result = browse(&mut c).await;

    let _ = browse_result.as_ref().map_err(|e| println!("{}", e));

    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;

    let close_result = c.close().await;

    browse_result?;
    close_result?;
    Ok(())
}

async fn wait_for_child(e: &mut Element, l: Locator<'_>) -> Result<Element, CmdError> {
    let mut found = e.find(l).await;
    while found.is_err() {
        sleep(Duration::from_millis(500)).await;
        found = e.find(l).await;
    }
    found
}

async fn scroll_wait_click(c: &mut Client, e: Element) -> Result<(), CmdError> {
    sleep(Duration::from_millis(100)).await;
    c.execute(r#"arguments[0].scrollIntoView({block: "center"});arguments[0]"#,
                   vec![serde_json::to_value(e.clone())?]).await?;
    e.click().await?;
    Ok(())
}

async fn browse(client: &mut Client) -> Result<()> {
    client.set_window_size(500, 1920).await?;
    client.goto("https://twitter.com/login").await?;
    sleep(Duration::from_millis(3000)).await;
    let url = client.current_url().await?;

    if url == "https://twitter.com/login".try_into()? {
        println!("logging in");
        client.find(Locator::Css("input[name=\"session[username_or_email]\"]")).await?
            .send_keys(&input::<String>().repeat_msg("Username: ").get()).await?;
        client.find(Locator::Css("input[name=\"session[password]\"]")).await?
            .send_keys(&prompt_password_stdout("Password: ")?).await?;
        client.find(Locator::Css("div[data-testid=\"LoginForm_Login_Button\"]")).await?
            .click().await?;
    }

    sleep(Duration::from_millis(1000)).await;

    let promoted_locate = Locator::Css(r#"div > svg > g > path[d="M20.75 2H3.25C2.007 2 1 3.007 1 4.25v15.5C1 20.993 2.007 22 3.25 22h17.5c1.243 0 2.25-1.007 2.25-2.25V4.25C23 3.007 21.993 2 20.75 2zM17.5 13.504c0 .483-.392.875-.875.875s-.875-.393-.875-.876V9.967l-7.547 7.546c-.17.17-.395.256-.62.256s-.447-.086-.618-.257c-.342-.342-.342-.896 0-1.237l7.547-7.547h-3.54c-.482 0-.874-.393-.874-.876s.392-.875.875-.875h5.65c.483 0 .875.39.875.874v5.65z"]"#);
    let timeline_locate = Locator::XPath(r#"//div[count(./div[.//div[@data-testid="tweet"]]) > 1]"#);
    let first_tweet_locate = Locator::XPath("./*[not(@seen)]");
    let next_tweet_locate = Locator::XPath("./following-sibling::*");
    let more_locate = Locator::Css(r#"div[data-testid="caret"]"#);
    let block_locate = Locator::Css(r#"div[data-testid="block"]"#);
    let block_confirm_locate = Locator::Css(r#"div[data-testid="confirmationSheetConfirm"]"#);
    let mut count: u64 = 0;
    let mut blocked: u64 = 0;
    let t_start = Instant::now();

    loop {
        if client.current_url().await? != "https://twitter.com/home".try_into()? {
            bail!("this isnt the home page")
        }
        let res:Result<(), CmdError> = try {
            let mut timeline = client.wait_for_find(timeline_locate).await?;
            sleep(Duration::from_millis(100)).await;
            let mut t = timeline.find(first_tweet_locate).await?;
            loop {
                let t_serialized = serde_json::to_value(&mut t)?;
                client.execute(r#"arguments[0].scrollIntoView({block: "center"});"#, vec![t_serialized.to_owned()]).await?;
                if t.find(promoted_locate).await.is_ok() {
                    println!("found promoted tweet: {}", t.text().await.unwrap_or_default());
                    let more = t.find(more_locate).await?;
                    scroll_wait_click(client, more).await?;
                    let block = client.wait_for_find(block_locate).await?;
                    scroll_wait_click(client, block).await?;
                    client.wait_for_find(block_confirm_locate).await?.click().await?;
                    println!("blocked");
                    blocked += 1;
                }
                client.execute(r#"arguments[0].setAttribute("seen", "")"#, vec![t_serialized]).await?;
                count += 1;
                if let Ok(next_t) = t.find(next_tweet_locate).await {
                    t = next_t;
                } else if let Ok(Ok(next_t)) = timeout(Duration::from_secs(10), wait_for_child(&mut timeline, first_tweet_locate)).await {
                    t = next_t;
                } else {
                    break;
                }
            }
            let t_dur_min = Instant::now().duration_since(t_start).as_secs_f64() / 60.0;
            println!("{:.2}min elapsed, read {} ({:.2}/min), blocked {} ({:.2}/min)", t_dur_min, count, count as f64 / t_dur_min, blocked, blocked as f64 / t_dur_min);
            println!("Reloading...");
            client.refresh().await?;
        };
        if let Err(CmdError::NoSuchElement(_)) = res {
            println!("Lost an element somewhere. Reloading...");
            client.refresh().await?;
        } else if res.is_err() {
            res?
        }
    }
}