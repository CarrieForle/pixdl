// Method inspired by ZipFile (https://gist.github.com/ZipFile/c9ebedb224406f4f11845ab700124362)
// Initially I thought OAuth is required to download Pixiv image,
// but I figured a way to get around it. Thanks pixivpy for 
// aspiration and this is left unused for once it might
// come true... But I would probably have to rewrite the whole
// thing anyway the moment it came.

use std::{error::Error, io::stdin};

use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};

use crate::pkce;

// This is interactive
pub(crate) async fn login(client: &Client) -> Result<PixivCredential, Box<dyn Error>> {
    let (code_verifier, code_challenge) = pkce::generate();
    const ORIGIN: &'static str = "https://app-api.pixiv.net/web/v1/login";
    const AUTH_TOKEN_URL: &'static str = "https://oauth.secure.pixiv.net/auth/token";
    const CLIENT_ID: &'static str = "MOBrBDS8blbauoSck0ZfDbtuzpyT";
    const CLIENT_SECRET: &'static str = "lsACyCD94FhDUtGTXi3QzcFE2uU1hqtDaKeqrdwj";
    const REDIRECT_URI: &'static str = "https://app-api.pixiv.net/web/v1/users/auth/pixiv/callback";

    let url = format!("{ORIGIN}?code_challenge={code_challenge}&code_challenge_method=S256&client=pixiv-android");

    println!("{url}");
    println!("Access the URL above and log in. You need to extract code after login. Here's how:");
    println!("1. Open DevTool and go to \"Network\" tab.");
    println!("2. Turn on \"Persist Logs\"");
    println!("3. Type \"callback?\" in the filter");
    println!("4. Log in to Pixiv");
    println!("5. Copy the URL that appears back to the console and hit enter.");
    println!("Note: Code's lifetime is extremely short. If the code expires you have to restart the login process.");
    println!();
    print!("Put the URL here = ");

    let mut oauth_code_url = String::new();

    stdin().read_line(&mut oauth_code_url)?;

    let code = if oauth_code_url.starts_with("https://") {
        let url = Url::parse(&oauth_code_url)?;
        let mut queries = url.query_pairs();

        let code = queries.find_map(|(key, value)| {
            if key == "code" {
                Some(value)
            } else {
                None
            }
        }).ok_or("Code is not found in URL.")?
        .into_owned();

        code
    } else {
        oauth_code_url
    };

    print!("code = {code}");

    let response = client.post(AUTH_TOKEN_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("client_secret", CLIENT_SECRET),
            ("code", &code),
            ("code_verifier", &code_verifier),
            ("grant_type", "authorization_code"),
            ("include_policy", "true"),
            ("redirect_uri", REDIRECT_URI)
        ])
        .header("User-Agent", "PixivAndroidApp/5.0.234 (Android 11; Pixel 5)")
        .send().await?
        .error_for_status()?;

    let js: serde_json::Value = response.json().await?;
    
    let invalid_json_err_msg = "Failed to extract access or refresh token from JSON";

    let access_token = js.as_object().ok_or(invalid_json_err_msg)?["access_token"]
        .as_str().ok_or(invalid_json_err_msg)?
        .to_string();
    
    let refresh_token = js.as_object().ok_or(invalid_json_err_msg)?["refresh_token"]
        .as_str().ok_or(invalid_json_err_msg)?
        .to_string();

    Ok(PixivCredential {
        access_token, 
        refresh_token,
    })
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PixivCredential {
    pub access_token: String,
    pub refresh_token: String,
}