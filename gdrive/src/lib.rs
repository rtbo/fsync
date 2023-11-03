use google_drive3::{oauth2, DriveHub};

use std::path;
use std::str;

fn token_cache_path() -> String {
    let p = path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let p = p.join("token_cache.json");
    p.into_os_string().into_string().unwrap()
}

pub async fn list_my_files() {
    let secret_json = unsafe { str::from_utf8_unchecked(include_bytes!("../client_secret.json")) };

    // Read application secret from a file. Sometimes it's easier to compile it directly into
    // the binary. The clientsecret file contains JSON like `{"installed":{"client_id": ... }}`
    let secret = oauth2::parse_application_secret(secret_json).expect("client_secret.json");

    // Create an authenticator that uses an InstalledFlow to authenticate. The
    // authentication tokens are persisted to a file named token_cache.json. The
    // authenticator takes care of caching tokens to disk and refreshing tokens once
    // they've expired.
    let token_cache = token_cache_path();
    let auth = oauth2::InstalledFlowAuthenticator::builder(
        secret,
        oauth2::InstalledFlowReturnMethod::HTTPRedirect,
    )
    .persist_tokens_to_disk(&token_cache)
    .build()
    .await
    .unwrap();

    let hub = DriveHub::new(
        hyper::Client::builder().build(
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_native_roots()
                .https_or_http()
                .enable_http1()
                .build(),
        ),
        auth,
    );

    let mut next_page_token: Option<String> = None;

    loop {
        // You can configure optional parameters by calling the respective setters at will, and
        // execute the final call using `doit()`.
        // Values shown here are possibly random and not representative !
        let mut query = hub
            .files()
            .list()
            // .team_drive_id("et")
            // .supports_team_drives(true)
            // .supports_all_drives(false)
            // .spaces("amet.")
            // .q("takimata")
            // .page_token("amet.")
            // .page_size(-20)
            // .order_by("ipsum")
            // .include_team_drive_items(true)
            // .include_permissions_for_view("Lorem")
            // .include_labels("gubergren")
            // .include_items_from_all_drives(false)
            // .drive_id("dolor")
            // .corpus("ea")
            // .corpora("ipsum")
            // .q("'root' in parents and mimeType='application/vnd.google-apps.folder'")
        ;
        if let Some(page_token) = next_page_token.as_deref() {
            query = query.page_token(page_token);
        }

        let result = query.doit().await.unwrap();

        next_page_token = result.1.next_page_token;

        if let Some(files) = result.1.files {
            for f in files {
                println!("{}", f.name.as_deref().unwrap_or("<no name>"));
                println!("   {:?}", f.parents);
            }
        }

        if next_page_token.is_none() {
            break;
        }
    }
}
