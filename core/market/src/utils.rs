pub mod response {
    use actix_web::HttpResponse;
    use serde::Serialize;
    use ya_client::model::ErrorMessage;

    pub fn ok<T: Serialize>(t: T) -> HttpResponse {
        HttpResponse::Ok().json(t)
    }

    pub fn created<T: Serialize>(t: T) -> HttpResponse {
        HttpResponse::Created().json(t)
    }

    pub fn no_content() -> HttpResponse {
        HttpResponse::NoContent().finish()
    }

    pub fn conflict() -> HttpResponse {
        HttpResponse::Conflict().finish()
    }

    pub fn gone() -> HttpResponse {
        HttpResponse::Gone().finish()
    }

    pub fn not_implemented() -> HttpResponse {
        HttpResponse::NotImplemented().json(ErrorMessage { message: None })
    }

    pub fn not_found() -> HttpResponse {
        HttpResponse::NotFound().json(ErrorMessage { message: None })
    }

    pub fn unauthorized() -> HttpResponse {
        HttpResponse::Unauthorized().json(ErrorMessage { message: None })
    }

    pub fn timeout() -> HttpResponse {
        HttpResponse::GatewayTimeout().json(ErrorMessage { message: None })
    }

    pub fn server_error(e: &impl ToString) -> HttpResponse {
        HttpResponse::InternalServerError().json(ErrorMessage::new(e.to_string()))
    }

    pub fn bad_request(e: &impl ToString) -> HttpResponse {
        HttpResponse::BadRequest().json(ErrorMessage::new(e.to_string()))
    }
}
