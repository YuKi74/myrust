use actix_web::{HttpRequest, HttpResponse, Responder};

pub struct Empty;
impl Responder for Empty {
    type Body = &'static str;

    fn respond_to(self, req: &HttpRequest) -> HttpResponse<Self::Body> {
        "".respond_to(req)
    }
}
