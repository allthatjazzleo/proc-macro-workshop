use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    parse_macro_input, Data, DeriveInput, Error, Expr, Fields, GenericArgument, Lit, Meta,
    PathArguments, Result, Token, Type,
};

#[proc_macro_derive(Builder, attributes(builder))]
pub fn derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    eprintln!("INPUT: {:#?}", input);

    expand(input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = input.ident;

    let builder_name = format_ident!("{}Builder", name);

    let builder_fields = builder_fields(&input.data);

    let builder_methods = builder_methods(&input.data);

    let builder_instance_fields = builder_instance_fields(&input.data);

    let instance_fields = instance_fields(&input.data);

    let expanded = quote! {
        pub struct #builder_name {
            #builder_fields
        }

        impl #builder_name {
            #builder_methods

            pub fn build(&mut self) -> std::result::Result<#name, std::boxed::Box<dyn std::error::Error>> {
                Ok(#name {
                    #instance_fields
                })
            }
        }

        impl #name {
            pub fn builder() -> #builder_name {
                #builder_name {
                    #builder_instance_fields
                }
            }
        }
    };

    Ok(expanded)
}

fn builder_fields(data: &Data) -> TokenStream {
    match *data {
        Data::Struct(ref data) => match data.fields {
            Fields::Named(ref fields) => {
                let recurse = fields.named.iter().map(|f| {
                    let name = &f.ident;
                    let ty = match &f.ty {
                        Type::Path(ref ty) => ty,
                        _ => unimplemented!(),
                    };
                    quote! {
                        #name: std::option::Option<#ty>
                    }
                });
                quote! {
                    #(#recurse,)*
                }
            }
            _ => unimplemented!(),
        },
        _ => unimplemented!(),
    }
}

fn builder_methods(data: &Data) -> TokenStream {
    match *data {
        Data::Struct(ref data) => match data.fields {
            Fields::Named(ref fields) => {
                let recurse = fields.named.iter().map(|f | {
                    let mut builder_each = None;
                    for attr in &f.attrs {
                        if attr.path().is_ident("builder") {
                            let nested = attr
                                .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                                .unwrap();
                            for meta in nested {
                                match meta {
                                    Meta::NameValue(meta) if meta.path.is_ident("each") => {
                                        if let Expr::Lit(expr) = meta.value {
                                            if let Lit::Str(lit) = expr.lit {
                                                builder_each = Some(lit.value())
                                            }
                                        }
                                    },
                                    Meta::NameValue(_) => {
                                        return Error::new(attr.meta.span(), "expected `builder(each = \"...\")`").to_compile_error();
                                    },
                                    _ => unimplemented!(),
                                }
                            };
                        }
                    }
                    let name = &f.ident;
                    match &f.ty {
                        Type::Path(ref ty) if ty.path.segments[0].ident == "Option" => {
                            match ty.path.segments[0].arguments {
                                PathArguments::AngleBracketed(ref arguments) => {
                                    match arguments.args[0] {
                                        GenericArgument::Type(ref ty) => {
                                            quote! {
                                                fn #name(&mut self, #name: #ty) -> &mut Self {
                                                    self.#name = Some(Some(#name));
                                                    self
                                                }
                                            }
                                        }
                                        _ => unimplemented!(),
                                    }
                                }
                                _ => unimplemented!(),
                            }
                        }
                        Type::Path(ref ty) if builder_each.is_some() => {
                            match ty.path.segments[0].arguments {
                                PathArguments::AngleBracketed(ref arguments) => {
                                    match arguments.args[0] {
                                        GenericArgument::Type(ref ty) => {
                                            let each_name = format_ident!("{}", builder_each.unwrap());
                                            quote! {
                                                fn #each_name(&mut self, #each_name: #ty) -> &mut Self {
                                                    if self.#name.is_none() {
                                                        self.#name = Some(Vec::new());
                                                    }
                                                    if let Some(ref mut v) = self.#name {
                                                            v.push(#each_name);
                                                    }
                                                    self
                                                }
                                            }
                                        }
                                        _ => unimplemented!(),
                                    }
                                }
                                _ => unimplemented!(),
                            }
                        }
                        _ => {
                            let ty = &f.ty;
                            quote! {
                                fn #name(&mut self, #name: #ty) -> &mut Self {
                                    self.#name = Some(#name);
                                    self
                                }
                            }
                        }
                    }
                });
                quote! {
                    #(#recurse)*
                }
            }
            _ => unimplemented!(),
        },
        _ => unimplemented!(),
    }
}

fn builder_instance_fields(data: &Data) -> TokenStream {
    match *data {
        Data::Struct(ref data) => match data.fields {
            Fields::Named(ref fields) => {
                let recurse = fields.named.iter().map(|f| {
                    let name = &f.ident;
                    quote! {
                        #name: None
                    }
                });
                quote! {
                    #(#recurse,)*
                }
            }
            _ => unimplemented!(),
        },
        _ => unimplemented!(),
    }
}

fn instance_fields(data: &Data) -> TokenStream {
    match *data {
        Data::Struct(ref data) => match data.fields {
            Fields::Named(ref fields) => {
                let recurse = fields.named.iter().map(|f| {
                    let name = &f.ident;
                    quote! {
                        #name: self.#name.to_owned().unwrap_or_default()
                    }
                });
                quote! {
                    #(#recurse,)*
                }
            }
            _ => unimplemented!(),
        },
        _ => unimplemented!(),
    }
}
