//! Diagnostic: send the legacy stash-tab request with the saved session and
//! print the HTTP status and the start of the body. Never prints the cookie.
//! Usage: cargo run -p rpt-stash --example probe_stash -- <league> [realm]
#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let league = args.next().expect("league");
    let realm = args.next().unwrap_or_else(|| "pc".into());
    let sid = rpt_stash::secrets::load_session().expect("keychain").expect("no saved session");
    let http = reqwest::Client::builder()
        .user_agent(format!("RustyPoeTrade/probe (contact: {})", rpt_stash::CONTACT))
        .build()
        .unwrap();
    let get = |url: String| http.get(url).header("Cookie", format!("POESESSID={}", sid.expose())).send();
    let html = get("https://www.pathofexile.com/my-account".into()).await.unwrap().text().await.unwrap();
    let mark = "/account/view-profile/";
    let name: String = html.find(mark).map(|i| html[i + mark.len()..].chars().take_while(|c| !matches!(c, '"' | '\'' | '?' | '/' | '<' | '>')).collect()).unwrap_or_default();
    println!("profile link name: {name:?}");
    let account = match name.rsplit_once('-') {
        Some((n, d)) if d.len() == 4 && d.chars().all(|c| c.is_ascii_digit()) => format!("{n}#{d}"),
        _ => name.clone(),
    };
    // extra tabs to parse: PROBE_TABS=1,3,4
    if let Ok(list) = std::env::var("PROBE_TABS") {
        for idx in list.split(',').filter_map(|x| x.trim().parse::<u32>().ok()) {
            let url = format!(
                "https://www.pathofexile.com/character-window/get-stash-items?accountName={}&realm={}&league={}&tabs=0&tabIndex={idx}",
                urlencode(&account), urlencode(&realm), urlencode(&league)
            );
            let r = get(url).await.unwrap();
            let status = r.status();
            let body = r.text().await.unwrap();
            match rpt_stash::model::parse_legacy_counts(&body) {
                Ok((_, i)) => println!("tab {idx}: {status} parsed OK, {i} items"),
                Err(e) => {
                    let col = e.column();
                    println!("tab {idx}: {status} PARSE ERROR: {e}\n  context: {}", &body[col.saturating_sub(150).min(body.len())..(col + 80).min(body.len())]);
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        }
        return;
    }
    for acc in [account.clone(), name.clone()] {
        let url = format!(
            "https://www.pathofexile.com/character-window/get-stash-items?accountName={}&realm={}&league={}&tabs=1&tabIndex=0",
            urlencode(&acc), urlencode(&realm), urlencode(&league)
        );
        let r = get(url).await.unwrap();
        let status = r.status();
        let ct = r.headers().get("content-type").map(|v| v.to_str().unwrap_or("").to_owned()).unwrap_or_default();
        let body = r.text().await.unwrap();
        let head: String = body.chars().take(300).collect();
        println!("accountName={acc:?}: {status} {ct} len={} :: {head}", body.len());
        if status.is_success() {
            if let Ok(out) = std::env::var("PROBE_OUT") {
                std::fs::write(&out, &body).unwrap();
            }
            match rpt_stash::model::parse_legacy_counts(&body) {
                Ok((t, i)) => println!("parsed OK: {t} tabs, {i} items"),
                Err(e) => {
                    println!("PARSE ERROR: {e}");
                    let col = e.column();
                    let start = col.saturating_sub(200);
                    println!("context: {}", &body[start.min(body.len())..(col + 100).min(body.len())]);
                }
            }
        }
    }
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}
