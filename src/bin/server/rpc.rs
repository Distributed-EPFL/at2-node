use tonic::Response;

tonic::include_proto!("at2");

use at2_server::At2;
pub use at2_server::At2Server;

#[derive(Default)]
pub struct Service {}

#[tonic::async_trait]
impl At2 for Service {
    async fn send_money(
        &self,
        _request: tonic::Request<SendMoneyRequest>,
    ) -> Result<tonic::Response<SendMoneyReply>, tonic::Status> {
        let reply = SendMoneyReply { request_id: vec![] };
        Ok(Response::new(reply))
    }
}
