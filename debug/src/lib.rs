use proc_macro2::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, Data, DeriveInput, Error, Expr, Fields, Lit, Meta, GenericParam, Generics, parse_quote,
    Result, Token,
};
use syn::visit::{self, Visit};
use std::collections::HashMap;

#[proc_macro_derive(CustomDebug, attributes(debug))]
pub fn derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    // eprintln!("INPUT: {:#?}", input);

    expand(input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn expand(input: DeriveInput) -> Result<TokenStream> {
    let input_cloned = input.clone();
    let generics = add_trait_bounds(input_cloned)?;
    let name = input.ident;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let debug_fields = debug_fields(&input.data);

    let expanded = quote! {
        // The generated impl.
        impl #impl_generics std::fmt::Debug for #name #ty_generics #where_clause {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
                f.debug_struct(stringify!(#name))    
                #debug_fields
                .finish()
            }
        }
    };

    Ok(expanded)
}

struct TypePathVisitor {
    generic_type_names: Vec<String>, // record all generic types `T`,`U`
    associated_types: HashMap<String, Vec<syn::TypePath>>, // record all associated types `T::Value` under generic type `T`
}

impl<'ast> Visit<'ast> for TypePathVisitor {
    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        
        if node.path.segments.len() >= 2 {
            let generic_type_name = node.path.segments[0].ident.to_string();
            if self.generic_type_names.contains(&generic_type_name) {
                self.associated_types.entry(generic_type_name).or_default().push(node.clone());
            }
        }

        visit::visit_type_path(self, node);
    }
}

fn get_generic_associated_types(input: &syn::DeriveInput) -> HashMap<String, Vec<syn::TypePath>> {
    let origin_generic_param_names = input.generics.params.iter().filter_map(|f| {
        if let syn::GenericParam::Type(ty) = f {
            return Some(ty.ident.to_string())
        }
        None
    }).collect();

    let mut visitor = TypePathVisitor {
        generic_type_names: origin_generic_param_names,
        associated_types: HashMap::new(),
    };

    visitor.visit_derive_input(input);
    visitor.associated_types
}

type StructFields = syn::punctuated::Punctuated<syn::Field,syn::Token!(,)>;
fn get_fields_from_derive_input(d: &syn::DeriveInput) -> syn::Result<&StructFields> {
    if let syn::Data::Struct(syn::DataStruct {
        fields: syn::Fields::Named(syn::FieldsNamed { ref named, .. }),
        ..
    }) = d.data{
        return Ok(named)
    }
    Err(syn::Error::new_spanned(d, "Must define on a Struct, not Enum".to_string()))
}


// Add a bound `T: Debug` to every type parameter T except PhantomData<T>.
fn add_trait_bounds(mut input: syn::DeriveInput) -> syn::Result<Generics> {
    if let Some(hatch) = get_struct_escape_hatch(&input) {
        input.generics.make_where_clause();
        hatch.iter().for_each(|hatch| {
            input.generics
                .where_clause
                .as_mut()
                .unwrap()
                .predicates
                .push(syn::parse_str(hatch.as_str()).unwrap());
        });
    } else {
        let fields = get_fields_from_derive_input(&input)?;

        let mut field_type_names = Vec::new();
        let mut phantomdata_type_param_names = Vec::new();
        for field in fields{
            if let Some(s) = get_field_type_name(field)? {
                field_type_names.push(s);
            }
            if let Some(s) = get_phantomdata_generic_type_name(field)? {
                phantomdata_type_param_names.push(s);
            }
        }

        let associated_types_map = get_generic_associated_types(&input);
        for param in &mut input.generics.params {
            if let GenericParam::Type(ref mut type_param) = *param {
                let type_param_name = type_param.ident.to_string(); 
                if phantomdata_type_param_names.contains(&type_param_name) && !field_type_names.contains(&type_param_name) {
                    continue;
                }
                if associated_types_map.contains_key(&type_param_name) && !field_type_names.contains(&type_param_name){
                    continue
                }
                type_param.bounds.push(parse_quote!(std::fmt::Debug));
            }
        }

        input.generics.make_where_clause();
        for (_, associated_types) in associated_types_map {
            for associated_type in associated_types {
                // let another = associated_type.clone();
                // let debug: proc_macro2::TokenStream = parse_quote!(#another);
                // eprintln!("associated_type: {}", debug.to_string());

                input.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(#associated_type: std::fmt::Debug));
            }
        }
    }

    Ok(input.generics)
}

fn get_struct_escape_hatch(input: &syn::DeriveInput) -> Option<Vec<String>> {
    let mut escape_hatch = Vec::new();
    for attr in &input.attrs {
        if attr.path().is_ident("debug") {
            if let Ok(nested) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) {
                let mut bounds = nested.iter().filter_map( |meta| {
                    if let Meta::NameValue(meta) = meta {
                        if meta.path.is_ident("bound") {
                            if let Expr::Lit(ref expr) = meta.value {
                                if let Lit::Str(ref lit) = expr.lit {
                                    return Some(lit.value())
                                }
                            }
                        }
                    }
                    None
                }).collect::<Vec<_>>();
                escape_hatch.append(&mut bounds);
            }
        }
    }
    if escape_hatch.is_empty() {
        return None
    }
    Some(escape_hatch)
}

fn get_field_type_name(field: &syn::Field) -> syn::Result<Option<String>> {
    if let syn::Type::Path(syn::TypePath{path: syn::Path{ref segments, ..}, ..}) = field.ty {
        if let Some(syn::PathSegment{ref ident,..}) = segments.last() {
            return Ok(Some(ident.to_string()))
        }
    }
    Ok(None)
}

fn get_phantomdata_generic_type_name(field: &syn::Field) -> syn::Result<Option<String>> {
    if let syn::Type::Path(syn::TypePath{path: syn::Path{ref segments, ..}, ..}) = field.ty {
        if let Some(syn::PathSegment{ref ident, ref arguments}) = segments.last() {
            if ident == "PhantomData" {
                if let syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments{args, ..}) = arguments {
                    if let Some(syn::GenericArgument::Type(syn::Type::Path( ref gp))) = args.first() {
                        if let Some(generic_ident) = gp.path.segments.first() {
                            return Ok(Some(generic_ident.ident.to_string()))
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

fn debug_fields(data: &Data) -> TokenStream {
    match *data {
        Data::Struct(ref data) => match data.fields {
            Fields::Named(ref fields) => {
                let recurse = fields.named.iter().map(|f| {
                    let name = &f.ident;
                    let mut debug_format = None;
                    for attr in &f.attrs {
                        if attr.path().is_ident("debug") {
                            let meta = attr.meta.clone();
                            match meta {
                                Meta::NameValue(meta) => {
                                    if let Expr::Lit(expr) = meta.value {
                                        if let Lit::Str(lit) = expr.lit {
                                            debug_format = Some(lit.value());
                                        }
                                    }
                                }
                                _ => unimplemented!(),
                            }
                        }
                    }
                    if let Some(debug_format) = debug_format {
                        quote! {
                            .field(stringify!(#name), &format_args!(#debug_format, &self.#name))
                        }
                    } else {
                        quote! {
                            .field(stringify!(#name), &self.#name)
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
