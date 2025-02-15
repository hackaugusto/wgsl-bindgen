use std::{borrow::Cow, usize};

use naga::StructMember;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Ident, Index};

use super::rust_type;
use crate::{
  bevy_util::demangle_splitting_mod_path_and_item, WgslTypeSerializeStrategy,
  WgslBindgenOption,
};

#[derive(Clone)]
pub struct RustStructMemberEntryPadding {
  pub pad_name: Ident,
  pub pad_size_tokens: TokenStream,
}

impl RustStructMemberEntryPadding {
  fn generate_member_instantiate(&self) -> TokenStream {
    let pad_name = &self.pad_name;
    let pad_size = &self.pad_size_tokens;
    quote!(#pad_name: [0; #pad_size])
  }

  fn generate_member_definition(&self) -> TokenStream {
    let pad_name = &self.pad_name;
    let pad_size = &self.pad_size_tokens;
    quote!(pub #pad_name: [u8; #pad_size])
  }
}

#[derive(Default)]
struct NagaToRustStructState<'a> {
  index: usize,
  members: Vec<RustStructMemberEntry<'a>>,
}

impl<'a> NagaToRustStructState<'a> {
  fn create_fold(
    naga_members: &'a [StructMember],
    naga_module: &'a naga::Module,
    options: &'a WgslBindgenOption,
    layout_size: usize,
    is_directly_sharable: bool,
  ) -> impl FnMut(NagaToRustStructState<'a>, &'a StructMember) -> NagaToRustStructState<'a>
  {
    let fold = move |mut state: NagaToRustStructState<'a>,
                     naga_member: &'a StructMember|
          -> NagaToRustStructState<'a> {
      let name_ident = Ident::new(naga_member.name.as_ref().unwrap(), Span::call_site());
      let naga_type = &naga_module.types[naga_member.ty];

      let rust_type = rust_type(naga_module, naga_type, &options);
      let is_rsa = rust_type.size.is_none();

      if is_rsa && state.index != naga_members.len() - 1 {
        panic!("Only the last field of a struct can be a runtime-sized array");
      }

      // check if we need padding bytes
      let padding = if is_rsa || !is_directly_sharable {
        None
      } else {
        let current_offset = naga_member.offset as usize;
        let next_offset = if state.index + 1 < naga_members.len() {
          naga_members[state.index + 1].offset as usize
        } else {
          layout_size
        };
        let rust_type = &rust_type;

        let pad_name = format!("_pad_{}", naga_member.name.clone().unwrap());
        let required_member_size = next_offset - current_offset;

        match rust_type.size_after_alignment() {
          Some(rust_type_size) if required_member_size == rust_type_size => None,
          _ => {
            let required_member_size = format!("0x{:X}", required_member_size);
            let member_size =
              syn::parse_str::<TokenStream>(&required_member_size).unwrap();

            let pad_name = Ident::new(&pad_name, Span::call_site());
            let pad_size_tokens =
              quote!(#member_size - core::mem::size_of::<#rust_type>());

            let padding = RustStructMemberEntryPadding {
              pad_name,
              pad_size_tokens,
            };

            Some(padding)
          }
        }
      };

      let entry = RustStructMemberEntry {
        name_ident: name_ident.clone(),
        naga_member,
        naga_type,
        rust_type: syn::Type::Verbatim(rust_type.tokens),
        is_rsa,
        padding,
      };

      state.index += 1;
      state.members.push(entry);
      state
    };

    fold
  }
}

pub struct RustStructMemberEntry<'a> {
  pub name_ident: Ident,
  pub naga_member: &'a naga::StructMember,
  pub naga_type: &'a naga::Type,
  pub rust_type: syn::Type,
  pub padding: Option<RustStructMemberEntryPadding>,
  pub is_rsa: bool,
}

impl<'a> RustStructMemberEntry<'a> {
  fn generate_member_instantiate(&self, other_struct_var_name: &Ident) -> TokenStream {
    let name = &self.name_ident;
    quote!(#name: #other_struct_var_name.#name)
  }

  fn generate_member_definition(&self) -> TokenStream {
    let name = &self.name_ident;
    let ty = &self.rust_type;
    quote!(pub #name: #ty)
  }

  fn generate_fn_new_param(&self) -> TokenStream {
    let name = &self.name_ident;
    let ty = &self.rust_type;
    quote!(#name: #ty)
  }

  fn from_naga(
    naga_members: &'a [naga::StructMember],
    naga_module: &'a naga::Module,
    options: &'a WgslBindgenOption,
    layout_size: usize,
    is_directly_sharable: bool,
  ) -> Vec<Self> {
    let state = naga_members.iter().fold(
      NagaToRustStructState::default(),
      NagaToRustStructState::create_fold(
        naga_members,
        naga_module,
        options,
        layout_size,
        is_directly_sharable,
      ),
    );
    state.members
  }
}

pub struct RustStructBuilder<'a> {
  name: Cow<'a, str>,
  members: Vec<RustStructMemberEntry<'a>>,
  is_host_sharable: bool,
  has_rts_array: bool,
  naga_module: &'a naga::Module,
  layout: naga::proc::TypeLayout,
  options: &'a WgslBindgenOption,
}

impl<'a> RustStructBuilder<'a> {
  fn name_ident(&self) -> Ident {
    Ident::new(self.name.as_ref(), Span::call_site())
  }

  fn is_directly_shareable(&self) -> bool {
    self.options.serialization_strategy == WgslTypeSerializeStrategy::Bytemuck
      && self.is_host_sharable
  }

  fn uses_generics_for_rts(&self) -> bool {
    self.has_rts_array
      && self.options.serialization_strategy == WgslTypeSerializeStrategy::Bytemuck
  }

  fn uses_padding(&self) -> bool {
    self.members.iter().any(|m| m.padding.is_some())
  }

  fn struct_name_in_usage_fragment(&self) -> TokenStream {
    let ident = self.name_ident();

    if self.uses_generics_for_rts() {
      quote!(#ident<N>)
    } else {
      quote!(#ident)
    }
  }

  fn struct_name_in_definition_fragment(&self) -> TokenStream {
    let ident = self.name_ident();

    if self.uses_generics_for_rts() {
      quote!(#ident<const N: usize>)
    } else {
      quote!(#ident)
    }
  }

  fn init_struct_name_in_usage_fragment(&self) -> TokenStream {
    let name = format!("{}Init", self.name);
    let ident = Ident::new(&name, Span::call_site());
    if self.uses_generics_for_rts() {
      quote!(#ident<N>)
    } else {
      quote!(#ident)
    }
  }

  fn init_struct_name_in_definition_fragment(&self) -> TokenStream {
    let name = format!("{}Init", self.name);
    let ident = Ident::new(&name, Span::call_site());
    if self.uses_generics_for_rts() {
      quote!(#ident<const N: usize>)
    } else {
      quote!(#ident)
    }
  }

  fn impl_trait_for_fragment(&self) -> TokenStream {
    if self.uses_generics_for_rts() {
      quote!(impl<const N:usize>)
    } else {
      quote!(impl)
    }
  }

  fn build_init_struct(&self) -> TokenStream {
    if !self.is_directly_shareable() || !self.uses_padding() {
      return quote!();
    }

    let impl_fragment = self.impl_trait_for_fragment();
    let struct_name_usage = self.struct_name_in_usage_fragment();
    let struct_name = self.name_ident();
    let init_struct_name_def = self.init_struct_name_in_definition_fragment();
    let init_struct_name_usage = self.init_struct_name_in_usage_fragment();

    let mut init_struct_members = vec![];
    let mut mem_assignments = vec![];

    let init_var_name = Ident::new("self", Span::call_site());

    for entry in self.members.iter() {
      init_struct_members.push(entry.generate_member_definition());
      mem_assignments.push(entry.generate_member_instantiate(&init_var_name));

      for pad in entry.padding.iter() {
        mem_assignments.push(pad.generate_member_instantiate())
      }
    }

    quote! {
      #[repr(C)]
      #[derive(Debug, PartialEq, Clone, Copy)]
      pub struct #init_struct_name_def {
        #(#init_struct_members),*
      }

      #impl_fragment #init_struct_name_usage {
        pub const fn const_into(&self) -> #struct_name_usage {
          #struct_name {
            #(#mem_assignments),*
          }
        }
      }

      #impl_fragment From<#init_struct_name_usage> for #struct_name_usage {
        fn from(data: #init_struct_name_usage) -> Self {
          data.const_into()
        }
      }
    }
  }

  fn build_fn_new(&self) -> TokenStream {
    let struct_name_usage = self.struct_name_in_usage_fragment();
    let impl_fragment = self.impl_trait_for_fragment();

    let mut non_padding_members = Vec::new();
    let mut member_assignments = Vec::new();

    for entry in &self.members {
      let name = &entry.name_ident;
      non_padding_members.push(entry.generate_fn_new_param());
      member_assignments.push(quote!(#name));

      for p in entry.padding.iter() {
        member_assignments.push(p.generate_member_instantiate())
      }
    }

    quote! {
      #impl_fragment #struct_name_usage {
        pub fn new(
          #(#non_padding_members),*
        ) -> Self {
          Self {
            #(#member_assignments),*
          }
        }
      }
    }
  }

  fn build_fields(&self) -> Vec<TokenStream> {
    let gctx = self.naga_module.to_ctx();
    let members = self
      .members
      .iter()
      .map(
        |RustStructMemberEntry {
           name_ident: name,
           rust_type,
           is_rsa: is_rts,
           naga_member: member,
           naga_type,
           padding,
         }| {
          let doc = if self.is_directly_shareable() {
            let offset = member.offset;
            let size = naga_type.inner.size(gctx);
            let ty_name = naga_type.inner.to_wgsl(&gctx);
            let doc =
              format!(" size: {}, offset: 0x{:X}, type: `{}`", size, offset, ty_name);

            quote!(#[doc = #doc])
          } else {
            quote!()
          };

          let runtime_size_attribute = if *is_rts
            && matches!(
              self.options.serialization_strategy,
              WgslTypeSerializeStrategy::Encase
            ) {
            quote!(#[size(runtime)])
          } else {
            quote!()
          };

          let mut qs = vec![quote! {
            #doc
            #runtime_size_attribute
            pub #name: #rust_type
          }];

          for padding in padding.iter() {
            qs.push(padding.generate_member_definition());
          }

          quote!(#(#qs), *)
        },
      )
      .collect::<Vec<_>>();

    members
  }

  fn build_derives(&self) -> Vec<TokenStream> {
    let mut derives = Vec::new();
    derives.push(quote!(Debug));
    derives.push(quote!(PartialEq));
    derives.push(quote!(Clone));

    match self.options.serialization_strategy {
      WgslTypeSerializeStrategy::Bytemuck => {
        derives.push(quote!(Copy));
      }
      WgslTypeSerializeStrategy::Encase => {
        if !self.has_rts_array {
          derives.push(quote!(Copy));
        }
        derives.push(quote!(encase::ShaderType));
      }
    }
    if self.options.derive_serde {
      derives.push(quote!(serde::Serialize));
      derives.push(quote!(serde::Deserialize));
    }
    derives
  }

  fn build_assert_layout(&self) -> TokenStream {
    let ident = self.name_ident();
    let struct_name = if self.uses_generics_for_rts() {
      quote!(#ident<1>) // test RTS with 1 element
    } else {
      quote!(#ident)
    };

    let assert_member_offsets: Vec<_> = self
      .members
      .iter()
      .map(|m| {
        let m = m.naga_member;
        let name = Ident::new(m.name.as_ref().unwrap(), Span::call_site());
        let rust_offset = quote!(std::mem::offset_of!(#struct_name, #name));
        let wgsl_offset = Index::from(m.offset as usize);
        quote!(assert!(#rust_offset == #wgsl_offset);)
      })
      .collect();

    if self.is_directly_shareable() {
      // Assert that the Rust layout matches the WGSL layout.
      // Enable for bytemuck since it uses the Rust struct's memory layout.

      // TODO: Does the Rust alignment matter if it's copied to a buffer anyway?
      let struct_size = Index::from(self.layout.size as usize);

      quote! {
        const _: () = {
          #(#assert_member_offsets)*
          assert!(std::mem::size_of::<#struct_name>() == #struct_size);
        };
      }
    } else {
      quote!()
    }
  }

  pub fn build(&self) -> TokenStream {
    let struct_name_def = self.struct_name_in_definition_fragment();
    let struct_name_usage = self.struct_name_in_usage_fragment();
    let impl_fragment = self.impl_trait_for_fragment();

    // Assume types used in global variables are host shareable and require validation.
    // This includes storage, uniform, and workgroup variables.
    // This also means types that are never used will not be validated.
    // Structs used only for vertex inputs do not require validation on desktop platforms.
    // Vertex input layout is handled already by setting the attribute offsets and types.
    // This allows vertex input field types without padding like vec3 for positions.
    let is_host_shareable = self.is_host_sharable;

    let has_rts_array = self.has_rts_array;
    let should_generate_padding = is_host_shareable
      && self.options.serialization_strategy == WgslTypeSerializeStrategy::Bytemuck;

    let derives = self.build_derives();

    let alignment = Index::from((self.layout.alignment * 1u32) as usize);
    let repr_c = if !has_rts_array {
      if should_generate_padding {
        quote!(#[repr(C, align(#alignment))])
      } else {
        quote!(#[repr(C)])
      }
    } else {
      quote!()
    };

    let ignore_case_tokens = 
      if self.name.chars().next().unwrap().is_lowercase() {
        quote!(#[allow(non_camel_case_types)])
      } else {
        quote!()
      };

    let fields = self.build_fields();
    let struct_new_fn = self.build_fn_new();
    let init_struct = self.build_init_struct();
    let assert_layout = self.build_assert_layout();

    let unsafe_bytemuck_pod_impl =
      if self.options.serialization_strategy == WgslTypeSerializeStrategy::Bytemuck {
        quote! {
          unsafe #impl_fragment bytemuck::Zeroable for #struct_name_usage {}
          unsafe #impl_fragment bytemuck::Pod for #struct_name_usage {}
        }
      } else {
        quote!()
      };

    quote! {
        #ignore_case_tokens
        #repr_c
        #[derive(#(#derives),*)]
        pub struct #struct_name_def {
            #(#fields),*
        }

        #struct_new_fn
        #unsafe_bytemuck_pod_impl
        #assert_layout
        #init_struct
    }
  }

  pub fn from_naga(
    naga_type: &'a naga::Type,
    naga_members: &'a [naga::StructMember],
    naga_module: &'a naga::Module,
    options: &'a WgslBindgenOption,
    layout: naga::proc::TypeLayout,
    is_directly_sharable: bool,
    is_host_sharable: bool,
    has_rts_array: bool,
  ) -> Self {
    let members = RustStructMemberEntry::from_naga(
      naga_members,
      naga_module,
      options,
      layout.size as usize,
      is_directly_sharable,
    );

    let name = naga_type.name.as_ref().unwrap().into();

    let mut builder = RustStructBuilder {
      name,
      members,
      is_host_sharable,
      naga_module,
      options: &options,
      has_rts_array,
      layout,
    };

    // we don't need full qualification here
    let (_, demangled_name) = demangle_splitting_mod_path_and_item(&builder.name);
    builder.name = demangled_name.into();
    builder
  }
}
