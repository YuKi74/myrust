pub struct Client {
    app_id: String,
    app_secret: String,
}

impl Client {
    pub fn new(app_id: String, app_secret: String) -> Self {
        Self {
            app_id,
            app_secret,
        }
    }
}
