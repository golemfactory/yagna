use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::error::{Error, ErrorBadRequest};
use actix_web::http::Uri;
use actix_web::middleware::Next;
use actix_web::HttpMessage;
use url::form_urlencoded;

const AUTH_TOKEN_QUERY_KEY: &str = "authToken";

/// A manager credential removed from the request URI before any inner
/// middleware (in particular the access logger) observes the request.
pub(super) enum QueryCredential {
    Token(String),
    Invalid,
}

/// Removes the legacy manager `authToken` query parameter before passing the
/// request to logging and authentication middleware.
///
/// The credential is kept only in a private request extension and is removed
/// by authentication middleware. Administrator credentials are never resolved
/// from this extension.
pub async fn sanitize_query_auth(
    mut req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    sanitize_request(&mut req)?;
    next.call(req).await
}

pub(super) fn sanitize_request(req: &mut ServiceRequest) -> Result<(), Error> {
    let (uri, credential) =
        sanitize_uri(req.uri()).map_err(|_| ErrorBadRequest("Invalid query string"))?;

    if let Some(credential) = credential {
        req.head_mut().uri = uri;
        req.extensions_mut().insert(credential);
    }

    Ok(())
}

fn sanitize_uri(uri: &Uri) -> Result<(Uri, Option<QueryCredential>), ()> {
    let Some(query) = uri.query() else {
        return Ok((uri.clone(), None));
    };

    let mut retained = Vec::new();
    let mut token = None;
    let mut token_count = 0usize;

    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        if key == AUTH_TOKEN_QUERY_KEY {
            token_count += 1;
            if token_count == 1 {
                token = Some(value.into_owned());
            }
        } else {
            retained.push((key.into_owned(), value.into_owned()));
        }
    }

    if token_count == 0 {
        return Ok((uri.clone(), None));
    }

    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in retained {
        serializer.append_pair(&key, &value);
    }
    let query = serializer.finish();
    let path_and_query = if query.is_empty() {
        uri.path().to_string()
    } else {
        format!("{}?{}", uri.path(), query)
    };

    let mut parts = uri.clone().into_parts();
    parts.path_and_query = Some(path_and_query.parse().map_err(|_| ())?);
    let sanitized = Uri::from_parts(parts).map_err(|_| ())?;
    let credential = match token_count {
        1 => QueryCredential::Token(token.expect("one token was collected")),
        _ => QueryCredential::Invalid,
    };

    Ok((sanitized, Some(credential)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_token_and_preserves_other_query_parameters() {
        let uri: Uri = "/activity-api/v1/_monitor?first=1&authToken=secret&message=hello%20world"
            .parse()
            .unwrap();

        let (uri, credential) = sanitize_uri(&uri).unwrap();

        assert_eq!(
            uri.to_string(),
            "/activity-api/v1/_monitor?first=1&message=hello+world"
        );
        match credential.unwrap() {
            QueryCredential::Token(token) => assert_eq!(token, "secret"),
            QueryCredential::Invalid => panic!("expected one query credential"),
        }
    }

    #[test]
    fn removes_encoded_and_duplicate_token_keys_without_selecting_one() {
        let uri: Uri = "/path?authToken=first&keep=value&auth%54oken=second"
            .parse()
            .unwrap();

        let (uri, credential) = sanitize_uri(&uri).unwrap();

        assert_eq!(uri.to_string(), "/path?keep=value");
        assert!(matches!(credential, Some(QueryCredential::Invalid)));
    }

    #[test]
    fn leaves_query_without_auth_token_byte_for_byte_unchanged() {
        let uri: Uri = "/path?message=hello%20world&flag".parse().unwrap();
        let (sanitized, credential) = sanitize_uri(&uri).unwrap();

        assert_eq!(sanitized, uri);
        assert!(credential.is_none());
    }
}
