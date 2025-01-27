use std::fmt::Debug;

use enum_map::{enum_map, Enum, EnumMap};
use proc_macro2::TokenStream;
use quote::quote;
use strum_macros::EnumIter;

use crate::{quote_gen::RustTypeInfo, WgslTypeSerializeStrategy};

/// The `WgslType` enum represents various WGSL types, such as vectors and matrices.
/// See [spec](https://www.w3.org/TR/WGSL/#alignment-and-size)
#[derive(Debug, Clone, Copy, Enum, Hash, PartialEq, Eq, EnumIter)]
pub enum WgslType {
  Vec2i,
  Vec3i,
  Vec4i,
  Vec2u,
  Vec3u,
  Vec4u,
  Vec2f,
  Vec3f,
  Vec4f,
  Vec2h,
  Vec3h,
  Vec4h,
  Mat2x2f,
  Mat2x3f,
  Mat2x4f,
  Mat3x2f,
  Mat3x3f,
  Mat3x4f,
  Mat4x2f,
  Mat4x3f,
  Mat4x4f,
  Mat2x2h,
  Mat2x3h,
  Mat2x4h,
  Mat3x2h,
  Mat3x3h,
  Mat3x4h,
  Mat4x2h,
  Mat4x3h,
  Mat4x4h,
}

impl WgslType {
  /// Returns the alignment and size of the WGSL type.
  pub const fn alignment_and_size(&self) -> (u8, usize) {
    use WgslType::*;
    match self {
      Vec2i | Vec2u | Vec2f => (8, 8),
      Vec2h => (4, 4),
      Vec3i | Vec3u | Vec3f => (16, 12),
      Vec3h => (8, 6),
      Vec4i | Vec4u | Vec4f => (16, 16),
      Vec4h => (8, 8),

      // AlignOf(vecR), SizeOf(array<vecR, C>)
      Mat2x2f => (8, 16),
      Mat2x2h => (4, 8),
      Mat3x2f => (8, 24),
      Mat3x2h => (4, 12),
      Mat4x2f => (8, 32),
      Mat4x2h => (4, 16),
      Mat2x3f => (16, 32),
      Mat2x3h => (8, 16),
      Mat3x3f => (16, 48),
      Mat3x3h => (8, 24),
      Mat4x3f => (16, 64),
      Mat4x3h => (8, 32),
      Mat2x4f => (16, 32),
      Mat2x4h => (8, 16),
      Mat3x4f => (16, 48),
      Mat3x4h => (8, 24),
      Mat4x4f => (16, 64),
      Mat4x4h => (8, 32),
    }
  }

  /// Checks if the WGSL type is a vector.
  pub const fn is_vector(&self) -> bool {
    match self {
      WgslType::Vec2i
      | WgslType::Vec3i
      | WgslType::Vec4i
      | WgslType::Vec2u
      | WgslType::Vec3u
      | WgslType::Vec4u
      | WgslType::Vec2f
      | WgslType::Vec3f
      | WgslType::Vec4f
      | WgslType::Vec2h
      | WgslType::Vec3h
      | WgslType::Vec4h => true,
      _ => false,
    }
  }

  /// Checks if the WGSL type is a matrix.
  pub const fn is_matrix(&self) -> bool {
    match self {
      WgslType::Mat2x2f
      | WgslType::Mat2x3f
      | WgslType::Mat2x4f
      | WgslType::Mat3x2f
      | WgslType::Mat3x3f
      | WgslType::Mat3x4f
      | WgslType::Mat4x2f
      | WgslType::Mat4x3f
      | WgslType::Mat4x4f
      | WgslType::Mat2x2h
      | WgslType::Mat2x3h
      | WgslType::Mat2x4h
      | WgslType::Mat3x2h
      | WgslType::Mat3x3h
      | WgslType::Mat3x4h
      | WgslType::Mat4x2h
      | WgslType::Mat4x3h
      | WgslType::Mat4x4h => true,
      _ => false,
    }
  }
}

pub type WgslTypeMap = EnumMap<WgslType, Option<TokenStream>>;

/// A trait for building `WgslType` to `TokenStream` map.
///
/// This map is used to convert built-in WGSL types into their corresponding
/// representations in the generated Rust code. The specific format used for
/// matrix and vector types can vary, and the generated types for the same WGSL
/// type may differ in size or alignment.
///
/// Implementations of this trait provide a `build` function that takes a
/// `WgslTypeSerializeStrategy` and returns an `WgslTypeMap`. 
pub trait WgslTypeMapBuild {
  /// Builds the `WgslTypeMap` based on the given serialization strategy.
  fn build(&self, strategy: WgslTypeSerializeStrategy) -> WgslTypeMap;
}

/// Provides an extension method for `WgslTypeMap` to convert WGSL types to `RustTypeInfo`.
pub(crate) trait WgslTypeMapExt {
  /// Gets the `RustTypeInfo` for the given WGSL type.
  fn get_rust_type_info(&self, wgsl_ty: WgslType) -> Option<RustTypeInfo>;
}

impl WgslTypeMapExt for WgslTypeMap {
  fn get_rust_type_info(&self, wgsl_ty: WgslType) -> Option<RustTypeInfo> {
    let (alignment_width, size) = wgsl_ty.alignment_and_size();
    let ty = self[wgsl_ty].as_ref()?.clone();
    let alignment = naga::proc::Alignment::from_width(alignment_width);
    Some(RustTypeInfo(ty, size, alignment))
  }
}

impl<T: WgslTypeMapBuild + 'static> From<T> for Box<dyn WgslTypeMapBuild> {
  fn from(value: T) -> Self {
    Box::new(value)
  }
}

impl Default for Box<dyn WgslTypeMapBuild> {
  fn default() -> Self {
    Box::new(WgslRustTypeMap)
  }
}

/// Rust types like `[f32; 4]` or `[[f32; 4]; 4]`.
#[derive(Clone)]
pub struct WgslRustTypeMap;

impl WgslTypeMapBuild for WgslRustTypeMap {
  fn build(&self, _: WgslTypeSerializeStrategy) -> WgslTypeMap {
    WgslTypeMap::default()
  }
}

/// `glam` types like `glam::Vec4` or `glam::Mat4`.
/// Types not representable by `glam` like `mat2x3<f32>` will use the output from [WgslRustTypeMap::map].
#[derive(Clone)]
pub struct GlamWgslTypeMap;

impl WgslTypeMapBuild for GlamWgslTypeMap {
  fn build(&self, serialize_strategy: WgslTypeSerializeStrategy) -> WgslTypeMap {
    let is_encase = serialize_strategy.is_encase();
    enum_map! {
      WgslType::Vec2i if is_encase => Some(quote!(glam::IVec2)),
      WgslType::Vec3i if is_encase => Some(quote!(glam::IVec3)),
      WgslType::Vec4i if is_encase => Some(quote!(glam::IVec4)),
      WgslType::Vec2u if is_encase => Some(quote!(glam::UVec2)),
      WgslType::Vec3u if is_encase => Some(quote!(glam::UVec3)),
      WgslType::Vec4u if is_encase => Some(quote!(glam::UVec4)),
      WgslType::Vec2f if is_encase => Some(quote!(glam::Vec2)),
      WgslType::Vec3f => Some(quote!(glam::Vec3A)),
      WgslType::Vec4f => Some(quote!(glam::Vec4)),
      WgslType::Mat2x2f if is_encase => Some(quote!(glam::Mat2)),
      WgslType::Mat3x3f => Some(quote!(glam::Mat3A)),
      WgslType::Mat4x4f => Some(quote!(glam::Mat4)),
      _ => None,
    }
  }
}

/// `nalgebra` types like `nalgebra::SVector<f64, 4>` or `nalgebra::SMatrix<f32, 2, 3>`.
#[derive(Clone)]
pub struct NalgebraWgslTypeMap;

impl WgslTypeMapBuild for NalgebraWgslTypeMap {
  fn build(&self, _: WgslTypeSerializeStrategy) -> WgslTypeMap {
    enum_map! {
      WgslType::Vec2i => Some(quote!(nalgebra::SVector<i32, 2>)),
      WgslType::Vec3i => Some(quote!(nalgebra::SVector<i32, 3>)),
      WgslType::Vec4i => Some(quote!(nalgebra::SVector<i32, 4>)),
      WgslType::Vec2u => Some(quote!(nalgebra::SVector<u32, 2>)),
      WgslType::Vec3u => Some(quote!(nalgebra::SVector<u32, 3>)),
      WgslType::Vec4u => Some(quote!(nalgebra::SVector<u32, 4>)),
      WgslType::Vec2f => Some(quote!(nalgebra::SVector<f32, 2>)),
      WgslType::Vec3f => Some(quote!(nalgebra::SVector<f32, 3>)),
      WgslType::Vec4f => Some(quote!(nalgebra::SVector<f32, 4>)),
      WgslType::Mat2x2f => Some(quote!(nalgebra::SMatrix<f32, 2, 2>)),
      WgslType::Mat2x3f => Some(quote!(nalgebra::SMatrix<f32, 3, 2>)),
      WgslType::Mat2x4f => Some(quote!(nalgebra::SMatrix<f32, 4, 2>)),
      WgslType::Mat3x2f => Some(quote!(nalgebra::SMatrix<f32, 2, 3>)),
      WgslType::Mat3x3f => Some(quote!(nalgebra::SMatrix<f32, 3, 3>)),
      WgslType::Mat3x4f => Some(quote!(nalgebra::SMatrix<f32, 4, 3>)),
      WgslType::Mat4x2f => Some(quote!(nalgebra::SMatrix<f32, 2, 4>)),
      WgslType::Mat4x3f => Some(quote!(nalgebra::SMatrix<f32, 3, 4>)),
      WgslType::Mat4x4f => Some(quote!(nalgebra::SMatrix<f32, 4, 4>)),
      _ => None,
    }
  }
}
