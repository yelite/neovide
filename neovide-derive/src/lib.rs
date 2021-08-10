use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, Attribute, Data, DataStruct, DeriveInput, Error, Ident, Lit, Meta, Field, MetaNameValue};

enum SettingType {
    Variable,
    Option,
}

struct SettingData {
    setting_type: SettingType,
    field_name: Ident,
    vim_name: String,
}

#[proc_macro_derive(SettingGroup, attributes(setting_prefix, name, opt))]
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
        match parse_setting_data(field, prefix.clone()) {
            Ok(setting_data) => build_variable_fragments(setting_data, &struct_name),
            Err(error) => error.to_compile_error().into()
        }
    });

    TokenStream::from(quote! {
        impl #struct_name {
            pub fn register() {
                let settings_struct: Self = Default::default();
                crate::settings::SETTINGS.set_global(&settings_struct);
                #(#fragments)*
            }
        }
    })
}

fn parse_setting_data(field: &Field, prefix: String) -> Result<SettingData, Error> {
    if field.attrs.len() > 1 {
        return Err(Error::new_spanned(field, "Field has multiple attributes"));
    }

    if let Some(field_name) = field.ident.as_ref() {
        if let Some(attribute) = field.attrs.first() {
            if let Ok(Meta::NameValue(MetaNameValue { lit: Lit::Str(name), .. })) = attribute.parse_meta() {
                if attribute.path.is_ident("opt") {
                    Ok(SettingData {
                        setting_type: SettingType::Option,
                        field_name: field_name.clone(),
                        vim_name: name.value(),
                    })
                } else {
                    Err(Error::new_spanned(attribute, format!("Field attribute with path {:?} not recognized", attribute.path.get_ident())))
                }
            } else {
                Err(Error::new_spanned(attribute, "Field attributes on SettingGroup must be name values"))
            }
        } else {
            let vim_name = format!("{}{}", prefix, field_name);
            Ok(SettingData {
                setting_type: SettingType::Variable,
                field_name: field_name.clone(),
                vim_name,
            })
        }
    } else {
        Err(Error::new_spanned(field.colon_token, "Expected named struct fields"))
    }
}

fn build_variable_fragments(SettingData { field_name, vim_name, .. }: SettingData, struct_name: &Ident) -> TokenStream2 {
    let output_stream = quote! {{
        fn update_func(value: rmpv::Value) {
            let mut setting_struct = crate::settings::SETTINGS.get_global::<#struct_name>();
            setting_struct.#field_name.from_value(value);
            crate::settings::SETTINGS.set(&setting_struct);
        }

        fn reader_func() -> rmpv::Value {
            let setting_struct = crate::settings::SETTINGS.get_global::<#struct_name>();
            setting_struct.#field_name.into()
        }

        crate::settings::SETTINGS.set_setting_handlers(
            #vim_name,
            update_func,
            reader_func
        );
    }};
    output_stream.into()
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
