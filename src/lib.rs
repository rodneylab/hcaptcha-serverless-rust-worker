use serde::Deserialize;
use std::collections::HashMap;
use worker::*;

mod utils;

fn log_request(req: &Request) {
    console_log!(
        "{} - [{}], located at: {:?}, within: {}",
        Date::now().to_string(),
        req.path(),
        req.cf().coordinates().unwrap_or_default(),
        req.cf().region().unwrap_or("unknown region".into())
    );
}

#[derive(Deserialize)]
struct CaptchaRequest {
    response: String,
}

//Expected response
// {
//  "success": true|false, // is the passcode valid, and does it meet security criteria you specified, e.g. sitekey?
//  "challenge_ts": timestamp, // timestamp of the challenge (ISO format yyyy-MM-dd'T'HH:mm:ssZZ)
//  "hostname": string, // the hostname of the site where the challenge was solved
//  "credit": true|false, // optional: whether the response will be credited
//  "error-codes": [...] // optional: any error codes
//  "score": float, // ENTERPRISE feature: a score denoting malicious activity.
//  "score_reason": [...] // ENTERPRISE feature: reason(s) for score.
// }
#[derive(Deserialize)]
struct HcaptchaResponse {
    success: bool,
}

fn preflight_response(headers: &worker::Headers, cors_origin: &str) -> Result<Response> {
    let origin = match headers.get("Origin").unwrap() {
        Some(value) => value,
        None => return Response::empty(),
    };
    let mut headers = worker::Headers::new();
    headers.set("Access-Control-Allow-Headers", "Content-Type")?;
    headers.set("Access-Control-Allow-Methods", "POST")?;

    for origin_element in cors_origin.split(',') {
        if origin.eq(origin_element) {
            headers.set("Access-Control-Allow-Origin", &origin)?;
            break;
        }
    }
    headers.set("Access-Control-Max-Age", "86400")?;
    Ok(Response::empty()
        .unwrap()
        .with_headers(headers)
        .with_status(204))
}

async fn verify_captcha(client_response: &str, secret: &str, sitekey: &str) -> Option<bool> {
    let mut map = HashMap::new();
    map.insert("response", client_response);
    map.insert("secret", secret);
    map.insert("sitekey", sitekey);
    let client = reqwest::Client::new();
    let response = match client
        .post("https://hcaptcha.com/siteverify")
        .form(&map)
        .send()
        .await
    {
        Ok(res) => res,
        Err(_) => return None,
    };
    match response.json::<HcaptchaResponse>().await {
        Ok(res) => Some(res.success),
        Err(_) => None,
    }
}

#[event(fetch)]
pub async fn main(req: Request, env: Env) -> Result<Response> {
    log_request(&req);

    // Optionally, get more helpful error messages written to the console in the case of a panic.
    utils::set_panic_hook();

    // Optionally, use the Router to handle matching endpoints, use ":name" placeholders, or "*name"
    // catch-alls to match on specific patterns. Alternatively, use `Router::with_data(D)` to
    // provide arbitrary data that will be accessible in each route via the `ctx.data()` method.
    let router = Router::new();

    // Add as many routes as your Worker needs! Each route will get a `Request` for handling HTTP
    // functionality and a `RouteContext` which you can use to  and get route parameters and
    // Environment bindings like KV Stores, Durable Objects, Secrets, and Variables.
    router
        .options("/post/data", |req, ctx| {
            preflight_response(req.headers(), &ctx.var("CORS_ORIGIN")?.to_string())
        })
        .post_async("/verify", |mut req, ctx| async move {
            let data: CaptchaRequest;
            match req.json().await {
                Ok(res) => data = res,
                Err(_) => return Response::error("Bad request", 400),
            }
            let hcaptcha_sitekey = ctx.var("HCAPTCHA_SITEKEY")?.to_string();
            let hcaptcha_secretkey = ctx.var("HCAPTCHA_SECRETKEY")?.to_string();
            match verify_captcha(&data.response, &hcaptcha_secretkey, &hcaptcha_sitekey).await {
                Some(value) => {
                    if value {
                        // you would proceed with request here
                        console_log!("User verified");
                    }
                    // We don't let the user know we think they are a bot if verify failed
                    Response::ok("Have a great day!")
                }
                // something went wrong - we don't know if the user is a bot or not
                None => Response::error("Error verifying user", 400),
            }
        })
        .run(req, env)
        .await
}
