use proc_macro::TokenStream;
use quote::quote;
use syn::DeriveInput;

#[proc_macro_derive(FromQuery)]
pub fn from_query(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();
    let name = &ast.ident;
    let gen = quote! {
        impl ::actix_web::FromRequest for #name {
            type Error = ::actix_web::Error;
            type Future = ::std::future::Ready<::std::result::Result<Self, Self::Error>>;

            fn from_request(req: &::actix_web::HttpRequest, payload: &mut ::actix_web::dev::Payload) -> Self::Future {
                let query = ::actix_web::web::Query::<Self>::from_request(req, payload).into_inner();
                ::std::future::ready(query.map(|q| q.into_inner()))
            }
        }
    };
    gen.into()
}

#[proc_macro_derive(FromBody)]
pub fn from_body(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();
    let name = &ast.ident;
    let gen = quote! {
        impl ::actix_web::FromRequest for #name {
            type Error = ::actix_web::Error;
            type Future = ::std::pin::Pin<Box<dyn ::std::future::Future<Output=::std::result::Result<Self, Self::Error>>>>;

            fn from_request(req: &::actix_web::HttpRequest, payload: &mut ::actix_web::dev::Payload) -> Self::Future {
                let fut = ::actix_web::web::Json::<Self>::from_request(req, payload);
                Box::pin(async move {
                    Ok(fut.await?.into_inner())
                })
            }
        }
    };
    gen.into()
}

#[proc_macro_derive(FromQueryValidated)]
pub fn from_query_validated(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();
    let name = &ast.ident;
    let gen = quote! {
        impl ::actix_web::FromRequest for #name {
            type Error = ::actix_web::Error;
            type Future = ::std::future::Ready<::std::result::Result<Self, Self::Error>>;

            fn from_request(req: &::actix_web::HttpRequest, payload: &mut ::actix_web::dev::Payload) -> Self::Future {
                let query = ::actix_web_validator::Query::<Self>::from_request(req, payload).into_inner();
                ::std::future::ready(query.map(|q| q.into_inner()))
            }
        }
    };
    gen.into()
}

#[proc_macro_derive(FromBodyValidated)]
pub fn from_body_validated(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();
    let name = &ast.ident;
    let gen = quote! {
        impl ::actix_web::FromRequest for #name {
            type Error = ::actix_web::Error;
            type Future = ::std::pin::Pin<Box<dyn ::std::future::Future<Output=::std::result::Result<Self, Self::Error>>>>;

            fn from_request(req: &::actix_web::HttpRequest, payload: &mut ::actix_web::dev::Payload) -> Self::Future {
                let fut = ::actix_web_validator::Json::<Self>::from_request(req, payload);
                Box::pin(async move {
                    Ok(fut.await?.into_inner())
                })
            }
        }
    };
    gen.into()
}