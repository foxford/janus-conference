use async_trait::async_trait;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {}

#[derive(Serialize)]
struct Response {}

#[async_trait]
impl super::Operation for Request {
    async fn call(&self, request: &super::Request) -> super::OperationResult {
        verb!("Calling signal.update operation"; {"handle_id": request.session_id()});
        Ok(Response {}.into())
    }

    fn is_handle_jsep(&self) -> bool {
        true
    }
}
