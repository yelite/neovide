use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, Attribute, Data, DataStruct, DeriveInput, Error, Ident, Lit, Meta, Field, MetaNameValue};

enum SettingType {
    Default,
    Global(String),
    Option(String),
}

#[proc_macro_derive(SettingGroup, attributes(setting_prefix, option, global))]
pub fn derive_setting_group(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let prefix = setting_prefix(input.attrs.as_ref())
        .map(|p| format!("{}_", p))
        .unwrap_or("".to_string());

    if let Data::Struct(data) = &input.data {
        derive_register_function(input.ident, prefix, data)
    } else {
        data_to_compile_error(input.data, "Derive macro expects a struct")
    }
}

fn derive_register_function(struct_name: Ident, prefix: String, data: &DataStruct) -> TokenStream {
    let fragments = data.fields.iter().map(|field| {
        if let Some(field_name) = field.ident.as_ref() {
            match parse_setting_type(field) {
                Ok(SettingType::Default) => {
                    let vim_setting_name = format!("{}{}", prefix, field_name);
                    build_variable_fragments(&vim_setting_name, field_name, &struct_name)
                },
                Ok(SettingType::Global(vim_setting_name)) => build_variable_fragments(&vim_setting_name, field_name, &struct_name),
                Ok(SettingType::Option(vim_option_name)) => build_option_fragments(&vim_option_name, field_name, &struct_name),
                Err(error) => error.to_compile_error().into(),
            }
        } else {
            Error::new_spanned(field.colon_token, "Expected named struct fields").to_compile_error().into()
        }
    });

    TokenStream::from(quote! {
        impl #struct_name {
            pub fn register() {
                let s: Self = Default::default();
                crate::settings::SETTINGS.set_global(&s);
                #(#fragments)*
            }
        }
    })
}

fn parse_setting_type(field: &Field) -> Result<SettingType, Error> {
    if field.attrs.len() > 1 {
        return Err(Error::new_spanned(field, "Field has multiple attributes"));
    }

    if let Some(attribute) = field.attrs.first() {
        if let Ok(Meta::NameValue(MetaNameValue { lit: Lit::Str(name), .. })) = attribute.parse_meta() {
            if attribute.path.is_ident("option") {
                Ok(SettingType::Option(name.value()))
            } else if attribute.path.is_ident("global") {
                Ok(SettingType::Global(name.value()))
            } else {
                Err(Error::new_spanned(attribute, format!("Field attribute with path {:?} not recognized", attribute.path)))
            }
        } else {
            Err(Error::new_spanned(attribute, "Field attributes on SettingGroup must be name values"))
        }
    } else {
        Ok(SettingType::Default)
    }
}

fn build_variable_fragments(vim_setting_name: &str, field_name: &Ident, struct_name: &Ident) -> TokenStream2 {
    let output_stream = quote! {{
        fn update_func(value: rmpv::Value) {
            let mut s = crate::settings::SETTINGS.get::<#struct_name>();
            s.#field_name.from_value(value);
            crate::settings::SETTINGS.set(&s);
        }

        fn reader_func() -> rmpv::Value {
            let s = crate::settings::SETTINGS.get::<#struct_name>();
            s.#field_name.into()
        }

        crate::settings::SETTINGS.set_setting_handlers(
            #vim_setting_name,
            update_func,
            reader_func
        );
    }};
    output_stream.into()
}

fn build_option_fragments(vim_option_name: &str, field_name: &Ident, struct_name: &Ident) -> TokenStream2 {

}

fn setting_prefix(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs.iter() {
        if let Ok(Meta::NameValue(name_value)) = attr.parse_meta() {
            if name_value.path.is_ident("setting_prefix") {
                if let Lit::Str(literal) = name_value.lit {
                    return Some(literal.value());
                }
            }
        }
    }
    None
}

fn data_to_compile_error(data: Data, message: &str) -> TokenStream {
    match data {
        Data::Struct(data) => Error::new_spanned(data.struct_token, message),
        Data::Enum(data) => Error::new_spanned(data.enum_token, message),
        Data::Union(data) => Error::new_spanned(data.union_token, message)
    }.to_compile_error().into()
}
