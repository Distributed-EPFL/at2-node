tonic::include_proto!("at2");

#[cfg(feature = "server")]
impl<T: at2_server::At2> tonic::transport::NamedService for at2_server::At2Server<T> {
    const NAME: &'static str = "at2.AT2";
}
