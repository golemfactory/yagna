pub mod error;
pub mod gsb_to_http;
pub mod http_to_gsb;
pub mod message;
pub mod response;

/*
Proxy http request through GSB
- create a HttpToGsbProxy
- pass a GsbHttpCallMessage
- receive the message and execute with GsbToHttpProxy
 */

pub const BUS_ID: &str = "/public/http-proxy";

#[cfg(test)]
mod tests {
    use mockito;

    #[actix_web::test]
    async fn gsb_proxy_execute() {
        // Mock server
        let mut server = mockito::Server::new();
        let url = server.url();

        server
            .mock("GET", "/endpoint")
            .with_status(201)
            .with_body("response")
            .create();

        let mut gsb_call = GsbToHttpProxy {
            base_url: url,
            method: "GET".to_string(),
            path: "/endpoint".to_string(),
            body: None,
        };

        let mut response_stream = gsb_call.pass();

        let mut v = vec![];
        while let Some(event) = response_stream.next().await {
            v.push(event.msg);
        }

        assert_eq!(vec!["response"], v);
    }

    #[actix_web::test]
    async fn gsb_proxy_invoke() {
        let gsb_call = HttpToGsbProxy {
            method: "GET".to_string(),
            path: "/endpoint".to_string(),
            body: None,
        };

        let stream = stream! {
            for i in 0..3 {
                let event = GsbHttpCallResponseEvent {
                    index: i,
                    timestamp: "timestamp".to_string(),
                    msg: format!("response {}", i)
                };
                let result = Ok(event);
                yield Ok(result)
            }
        };
        pin!(stream);
        let mut response_stream = gsb_call.pass(|_| stream);

        let mut v = vec![];
        while let Some(event) = response_stream.next().await {
            if let Ok(event) = event {
                v.push(event);
            }
        }

        assert_eq!(vec!["response 0", "response 1", "response 2"], v);
    }
}
