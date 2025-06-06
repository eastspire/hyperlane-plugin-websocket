use crate::*;

impl<'a> BroadcastType<'a> {
    pub fn get_key(broadcast_type: BroadcastType) -> String {
        match broadcast_type {
            BroadcastType::PointToPoint(key1, key2) => {
                let (first_key, second_key) = if key1 <= key2 {
                    (key1, key2)
                } else {
                    (key2, key1)
                };
                format!("{}-{}-{}", POINT_TO_POINT_KEY, first_key, second_key)
            }
            BroadcastType::PointToGroup(key) => {
                format!("{}-{}", POINT_TO_GROUP_KEY, key)
            }
        }
    }
}

impl WebSocket {
    pub fn new() -> Self {
        Self {
            broadcast_map: BroadcastMap::default(),
        }
    }

    fn subscribe_unwrap_or_insert(
        &self,
        broadcast_type: BroadcastType,
    ) -> BroadcastMapReceiver<Vec<u8>> {
        let key: String = BroadcastType::get_key(broadcast_type);
        self.broadcast_map.subscribe_unwrap_or_insert(&key)
    }

    fn point_to_point(&self, key1: &str, key2: &str) -> BroadcastMapReceiver<Vec<u8>> {
        self.subscribe_unwrap_or_insert(BroadcastType::PointToPoint(key1, key2))
    }

    fn point_to_group(&self, key: &str) -> BroadcastMapReceiver<Vec<u8>> {
        self.subscribe_unwrap_or_insert(BroadcastType::PointToGroup(key))
    }

    pub fn receiver_count<'a>(&self, broadcast_type: BroadcastType<'a>) -> OptionReceiverCount {
        let key: String = BroadcastType::get_key(broadcast_type);
        self.broadcast_map.receiver_count(&key)
    }

    pub fn pre_decrement_receiver_count<'a>(
        &self,
        broadcast_type: BroadcastType<'a>,
    ) -> OptionReceiverCount {
        self.receiver_count(broadcast_type)
            .map(|count| (count - 1).max(0))
    }

    pub fn send<'a, T>(
        &self,
        broadcast_type: BroadcastType<'a>,
        data: T,
    ) -> BroadcastMapSendResult<Vec<u8>>
    where
        T: Into<Vec<u8>>,
    {
        let key: String = BroadcastType::get_key(broadcast_type);
        self.broadcast_map.send(&key, data.into())
    }

    pub async fn run<'a, F1, Fut1, F2, Fut2, F3, Fut3>(
        &self,
        ctx: &Context,
        buffer_size: usize,
        broadcast_type: BroadcastType<'a>,
        request_handler: F1,
        on_sended: F2,
        on_client_closed: F3,
    ) where
        F1: FuncWithoutPin<Fut1>,
        Fut1: Future<Output = ()> + Send + 'static,
        F2: FuncWithoutPin<Fut2>,
        Fut2: Future<Output = ()> + Send + 'static,
        F3: FuncWithoutPin<Fut3>,
        Fut3: Future<Output = ()> + Send + 'static,
    {
        let mut receiver: Receiver<Vec<u8>> = match broadcast_type {
            BroadcastType::PointToPoint(key1, key2) => self.point_to_point(key1, key2),
            BroadcastType::PointToGroup(key) => self.point_to_group(key),
        };
        let key: String = BroadcastType::get_key(broadcast_type);
        let result_handle = || async {
            ctx.aborted().await;
            ctx.closed().await;
        };
        loop {
            tokio::select! {
                request_res = ctx.ws_request_from_stream(buffer_size) => {
                    let mut need_break = false;
                    if request_res.is_ok() {
                        request_handler(ctx.clone()).await;
                    } else {
                        need_break = true;
                        on_client_closed(ctx.clone()).await;
                    }
                    let body: ResponseBody = ctx.get_response_body().await;
                    let send_res: BroadcastMapSendResult<_> = self.broadcast_map.send(&key, body);
                    on_sended(ctx.clone()).await;
                    if need_break || send_res.is_err() {
                        break;
                    }
                },
                msg_res = receiver.recv() => {
                    if let Ok(msg) = msg_res {
                        if ctx.send_response_body(msg).await.is_err() || ctx.flush().await.is_err() {
                            break;
                        }
                    } else {
                        break;
                    }
               }
            }
        }
        result_handle().await;
    }
}
