pub fn in_k8s() -> bool {
    std::env::var("KUBERNETES_SERVICE_HOST").is_ok()
}
