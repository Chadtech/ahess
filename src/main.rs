mod ahess_error;
mod new_db_change;

use crate::ahess_error::AhessError;
use actix_web::{middleware, web, App, HttpRequest, HttpServer};
use clap::Parser;

async fn index(req: HttpRequest) -> &'static str {
    println!("REQ: {req:?}");
    "Hello world!"
}

#[derive(Debug, Parser)]
#[clap(author = "ct", version = "0.1", about = "Audio Generation")]
enum Args {
    NewDbChange { change_name: String },
}

#[actix_web::main]
async fn main() -> Result<(), AhessError> {
    let args = Args::parse();

    match args {
        Args::NewDbChange { change_name } => {
            new_db_change::run(change_name).map_err(|err| AhessError::NewDbChangeError(err))?;
        }
    }

    // std::thread::spawn(move || loop {
    //     std::thread::sleep(std::time::Duration::from_secs(1));
    // });
    //
    // HttpServer::new(|| {
    //     App::new()
    //         .service(web::resource("/index.html").to(|| async { "Hello world!" }))
    //         .service(web::resource("/").to(index))
    // })
    // .bind(("127.0.0.1", 9841))?
    // .run()
    // .await

    Ok(())
}

#[cfg(test)]
mod tests {
    use actix_web::{body::to_bytes, dev::Service, http, test, web, App, Error};

    use super::*;

    #[actix_web::test]
    async fn test_index() -> Result<(), Error> {
        let app = App::new().route("/", web::get().to(index));
        let app = test::init_service(app).await;

        let req = test::TestRequest::get().uri("/").to_request();
        let resp = app.call(req).await?;

        assert_eq!(resp.status(), http::StatusCode::OK);

        let response_body = resp.into_body();
        assert_eq!(to_bytes(response_body).await?, r##"Hello world!"##);

        Ok(())
    }
}
