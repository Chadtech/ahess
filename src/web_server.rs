use crate::ahess_error::AhessError;
use crate::db_pool;
use actix_web::{web, App, HttpRequest, HttpServer};

async fn index(data: web::Data<Model>, req: HttpRequest) -> &'static str {
    println!("REQ: {req:?}");
    "Hello world!"
}

async fn generate(data: web::Data<Model>, req: HttpRequest) -> &'static str {
    println!("REQ: {req:?}");
    "Hello world!"
}

struct Model {
    sqlx: sqlx::Pool<sqlx::Postgres>,
}

pub async fn run() -> Result<(), AhessError> {
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || loop {
        let r = tx.send("Hello from a thread!").unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
    });

    std::thread::spawn(move || {
        for r in rx {
            println!("Got: {}", r);
        }
    });

    let model = Model {
        sqlx: db_pool::make().await?,
    };

    let web_data = web::Data::new(model);

    HttpServer::new(move || {
        App::new()
            .app_data(web_data.clone())
            .service(web::resource("/index.html").to(|| async { "Hello world!" }))
            .service(web::resource("/").to(index))
            .service(web::resource("/generate").to(generate))
    })
    .bind(("127.0.0.1", 9841))
    .map_err(|err| AhessError::WebServerError(err))?
    .run()
    .await
    .map_err(|err| AhessError::WebServerError(err))?;

    Ok(())
}

#[cfg(test)]
mod web_server_tests {
    use actix_web::{body::to_bytes, dev::Service, http, test, web, App, Error};

    use super::*;

    #[actix_web::test]
    async fn test_index() -> Result<(), Error> {
        let model = Model {
            sqlx: db_pool::make().await.unwrap(),
        };

        let web_data = web::Data::new(model);

        let app = App::new()
            .app_data(web_data.clone())
            .route("/", web::get().to(index));
        let app = test::init_service(app).await;

        let req = test::TestRequest::get().uri("/").to_request();
        let resp = app.call(req).await?;

        assert_eq!(resp.status(), http::StatusCode::OK);

        let response_body = resp.into_body();
        assert_eq!(to_bytes(response_body).await?, r##"Hello world!"##);

        Ok(())
    }
}
