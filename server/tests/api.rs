//! Prova completa dell'API su un server avviato al volo (senza TLS, con
//! utenti di prova): accesso, chat, allegati a blocchi, consegna istantanea.

use std::time::Duration;

use futures::StreamExt;
use postamon_interno::config::{Config, LocalUser};
use serde_json::{Value, json};

async fn start() -> String {
    start_in(std::env::temp_dir().join(format!("postamon-interno-test-{}-{}", std::process::id(), rand_suffix())))
        .await
}

async fn start_in(dir: std::path::PathBuf) -> String {
    let user = |u: &str, n: &str| LocalUser { username: u.into(), display_name: n.into(), password: format!("pw-{u}") };
    let cfg = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        data_dir: dir,
        max_file_mb: 1,
        tls: false,
        imap: None,
        ldap: None,
        local_users: vec![user("alice", "Alice Rossi"), user("bob", "Bob Bianchi"), user("carla", "Carla Verdi")],
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(postamon_interno::serve(cfg, listener));
    format!("127.0.0.1:{}", addr.port())
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
}

struct Client {
    http: reqwest::Client,
    base: String,
    token: String,
    id: i64,
}

async fn login(addr: &str, user: &str, password: &str) -> Result<Client, u16> {
    let http = reqwest::Client::new();
    let base = format!("http://{addr}/api");
    let res = http
        .post(format!("{base}/login"))
        .json(&json!({ "username": user, "password": password }))
        .send()
        .await
        .unwrap();
    if !res.status().is_success() {
        return Err(res.status().as_u16());
    }
    let v: Value = res.json().await.unwrap();
    Ok(Client {
        token: v["token"].as_str().unwrap().into(),
        id: v["user"]["id"].as_i64().unwrap(),
        http,
        base,
    })
}

impl Client {
    async fn get(&self, path: &str) -> (u16, Value) {
        let res = self.http.get(format!("{}{path}", self.base)).bearer_auth(&self.token).send().await.unwrap();
        let status = res.status().as_u16();
        (status, res.json().await.unwrap_or(Value::Null))
    }

    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let res =
            self.http.post(format!("{}{path}", self.base)).bearer_auth(&self.token).json(&body).send().await.unwrap();
        let status = res.status().as_u16();
        (status, res.json().await.unwrap_or(Value::Null))
    }

    async fn put_chunk(&self, id: &str, offset: u64, data: &'static [u8]) -> (u16, Value) {
        let res = self
            .http
            .put(format!("{}/uploads/{id}?offset={offset}", self.base))
            .bearer_auth(&self.token)
            .body(data)
            .send()
            .await
            .unwrap();
        let status = res.status().as_u16();
        (status, res.json().await.unwrap_or(Value::Null))
    }
}

#[tokio::test]
async fn chat_with_attachment_end_to_end() {
    let addr = start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(login(&addr, "alice", "sbagliata").await.err(), Some(401));
    let alice = login(&addr, "AZIENDA\\Alice", "pw-alice").await.unwrap();
    let bob = login(&addr, "bob", "pw-bob").await.unwrap();
    let carla = login(&addr, "carla", "pw-carla").await.unwrap();

    // Bob è collegato al canale istantaneo.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/ws?token={}", bob.token)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_, users) = alice.get("/users").await;
    let bob_entry = users.as_array().unwrap().iter().find(|u| u["username"] == "bob").unwrap();
    assert_eq!(bob_entry["displayName"], "Bob Bianchi");
    assert_eq!(bob_entry["online"], true);

    // Chat a due: la seconda richiesta restituisce la stessa.
    let (status, conv) = alice.post("/conversations", json!({ "memberIds": [bob.id] })).await;
    assert_eq!(status, 200);
    let conv_id = conv["id"].as_i64().unwrap();
    let (_, again) = alice.post("/conversations", json!({ "memberIds": [bob.id, alice.id] })).await;
    assert_eq!(again["id"].as_i64(), Some(conv_id));

    // Allegato in due blocchi, con una ripresa dal punto giusto.
    let (_, up) = alice.post("/uploads", json!({ "name": "C:\\temp\\listino.pdf", "mime": "application/pdf", "size": 10 })).await;
    let file_id = up["id"].as_str().unwrap().to_string();
    assert_eq!(alice.put_chunk(&file_id, 0, b"abcd").await, (200, json!({ "received": 4 })));
    assert_eq!(alice.put_chunk(&file_id, 0, b"abcd").await, (409, json!({ "received": 4 })));
    assert_eq!(alice.get(&format!("/uploads/{file_id}")).await.1["received"], 4);
    // Prima che sia completo non si può allegare.
    let (status, _) = alice.post(&format!("/conversations/{conv_id}/messages"), json!({ "fileIds": [file_id] })).await;
    assert_eq!(status, 400);
    assert_eq!(alice.put_chunk(&file_id, 4, b"efghij").await, (200, json!({ "received": 10 })));
    // Oltre il limite configurato (1 MB) il caricamento non parte.
    let (status, _) = alice.post("/uploads", json!({ "name": "grande.zip", "size": 5_000_000 })).await;
    assert_eq!(status, 413);

    let (status, msg) = alice
        .post(&format!("/conversations/{conv_id}/messages"), json!({ "body": "Ciao Bob, ecco il listino", "fileIds": [file_id] }))
        .await;
    assert_eq!(status, 200);
    assert_eq!(msg["files"][0]["name"], "listino.pdf");

    // Bob lo riceve subito.
    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let frame = ws.next().await.unwrap().unwrap();
            if let tokio_tungstenite::tungstenite::Message::Text(text) = frame {
                let v: Value = serde_json::from_str(&text).unwrap();
                if v["type"] == "message" {
                    return v;
                }
            }
        }
    })
    .await
    .expect("messaggio istantaneo");
    assert_eq!(event["message"]["body"], "Ciao Bob, ecco il listino");

    let (_, convs) = bob.get("/conversations").await;
    assert_eq!(convs[0]["unread"], 1);
    assert_eq!(convs[0]["lastMessage"]["senderId"], alice.id);

    // Download con ripresa (Range) per chi è nella chat, niente per gli altri.
    let res = bob
        .http
        .get(format!("{}/files/{file_id}", bob.base))
        .bearer_auth(&bob.token)
        .header("Range", "bytes=4-")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 206);
    assert_eq!(res.headers()["content-range"], "bytes 4-9/10");
    assert_eq!(res.text().await.unwrap(), "efghij");
    assert_eq!(carla.get(&format!("/files/{file_id}")).await.0, 404);
    assert_eq!(carla.get(&format!("/conversations/{conv_id}/messages")).await.0, 403);

    let (status, _) =
        bob.post(&format!("/conversations/{conv_id}/read"), json!({ "messageId": msg["id"].as_i64().unwrap() })).await;
    assert_eq!(status, 204);
    assert_eq!(bob.get("/conversations").await.1[0]["unread"], 0);

    // Gruppo con titolo.
    let (_, group) =
        carla.post("/conversations", json!({ "memberIds": [alice.id, bob.id], "title": "Negozio Arconate" })).await;
    assert_eq!(group["isGroup"], true);
    assert_eq!(group["members"].as_array().unwrap().len(), 3);

    let (_, history) = bob.get(&format!("/conversations/{conv_id}/messages?limit=10")).await;
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(bob.post("/logout", json!({})).await.0, 204);
    assert_eq!(bob.get("/me").await.0, 401);
}

#[tokio::test]
async fn serves_postamon_updates() {
    let dir = std::env::temp_dir().join(format!("postamon-interno-agg-{}-{}", std::process::id(), rand_suffix()));
    let addr = start_in(dir.clone()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let http = reqwest::Client::new();
    let get = |path: &str| http.get(format!("http://{addr}/api/aggiornamenti/{path}")).send();

    // Nessuna versione pubblicata: niente aggiornamenti.
    assert_eq!(get("latest.json").await.unwrap().status().as_u16(), 204);

    let updates = dir.join("aggiornamenti");
    std::fs::create_dir_all(&updates).unwrap();
    std::fs::write(updates.join("Postamon_0.2.0_x64-setup.exe"), b"installer").unwrap();
    std::fs::write(
        updates.join("latest.json"),
        json!({
            "version": "0.2.0",
            "platforms": { "windows-x86_64": { "signature": "firma", "url": "Postamon_0.2.0_x64-setup.exe" } }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(dir.join("interno.sqlite.copia"), b"segreto").unwrap();

    // L'indirizzo dell'installer prende il nome con cui si è raggiunto il server.
    let manifest: Value = get("latest.json").await.unwrap().json().await.unwrap();
    let url = manifest["platforms"]["windows-x86_64"]["url"].as_str().unwrap();
    assert_eq!(url, format!("http://{addr}/api/aggiornamenti/Postamon_0.2.0_x64-setup.exe"));
    assert_eq!(manifest["platforms"]["windows-x86_64"]["signature"], "firma");

    let res = http.get(url).send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(res.bytes().await.unwrap().as_ref(), b"installer");

    // Solo i file della cartella degli aggiornamenti.
    assert_eq!(get("..%2Finterno.sqlite.copia").await.unwrap().status().as_u16(), 404);
    assert_eq!(get("mancante.exe").await.unwrap().status().as_u16(), 404);
}
