use actix_web::{dev::Payload, FromRequest, HttpRequest};
use std::future::Future;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DataManager<T>(Arc<RwLock<Arc<T>>>);

impl<T> DataManager<T> {
    pub fn new(data: T) -> Self {
        Self(Arc::new(RwLock::new(Arc::new(data))))
    }
    pub async fn get(&self) -> Arc<T> {
        self.0.read().await.clone()
    }
    pub async fn replace(&self, data: T) {
        let mut d = self.0.write().await;
        *d = Arc::new(data);
    }
}

impl<T> Clone for DataManager<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[derive(Clone)]
pub struct Data<T>(Arc<T>);

impl<T> Deref for Data<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: 'static> FromRequest for Data<T> {
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output=Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let mng = req.app_data::<DataManager<T>>()
            .map(|mng| mng.clone());
        Box::pin(async move {
            match mng {
                None => Err(actix_web::error::ErrorInternalServerError("data manager not set")),
                Some(mng) => Ok(Data(mng.get().await))
            }
        })
    }
}
