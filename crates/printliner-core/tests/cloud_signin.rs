//! The REST client against a mock server: the password sign-in, the email
//! code step, the MQTT username, the device list, and how errors come back.

use printliner_core::{Client, CloudError, Login};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const UA: &str = "PrintLiner/test";

#[tokio::test]
async fn password_then_email_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/user-service/user/login"))
        .and(body_json(json!({ "account": "a@b.c", "password": "pw" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({ "accessToken": "", "refreshToken": "", "loginType": "verifyCode" }),
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/user-service/user/sendemail/code"))
        .and(body_json(json!({ "email": "a@b.c", "type": "codeLogin" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/user-service/user/login"))
        .and(body_json(json!({ "account": "a@b.c", "code": "123456" })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "accessToken": "tok", "expiresIn": 3600 })),
        )
        .mount(&server)
        .await;

    let client = Client::with_base(server.uri(), UA);
    assert_eq!(client.login("a@b.c", "pw").await, Ok(Login::EmailCode));
    client.send_email_code("a@b.c").await.unwrap();
    let Login::Token(token) = client.login_with_code("a@b.c", " 123456 ").await.unwrap() else {
        panic!("expected a token");
    };
    assert_eq!(token.access_token, "tok");
    assert!(token.expires_at.is_some());
}

#[tokio::test]
async fn username_devices_and_tasks_send_the_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/design-user-service/my/preference"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "uid": 1234567 })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/iot-service/api/user/bind"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("fixtures/bind.json")),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/user-service/my/tasks"))
        .and(query_param("deviceId", "00M00A000000001"))
        .and(query_param("limit", "50"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("fixtures/tasks.json")),
        )
        .mount(&server)
        .await;

    let client = Client::with_base(server.uri(), UA);
    assert_eq!(client.mqtt_username("tok").await.unwrap(), "u_1234567");
    assert_eq!(client.devices("tok").await.unwrap().len(), 2);
    assert_eq!(
        client
            .tasks("tok", "00M00A000000001", 50)
            .await
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/user-service/user/login"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({ "code": 1, "message": "Bad email" })),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/iot-service/api/user/bind"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/design-user-service/my/preference"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let client = Client::with_base(server.uri(), UA);
    assert_eq!(
        client.login("x", "y").await,
        Err(CloudError::Rejected("Bad email".into()))
    );
    assert_eq!(client.devices("old").await, Err(CloudError::Unauthorized));
    assert_eq!(
        client.mqtt_username("t").await,
        Err(CloudError::Server(503))
    );
    let nowhere = Client::with_base("http://127.0.0.1:9", UA);
    assert!(matches!(
        nowhere.devices("t").await,
        Err(CloudError::Network(_))
    ));
}
