#import "../more-shader-files/reachme" as reachme 
#import types::{Scalars, VectorsU32, VectorsI32, VectorsF32, MatricesF32, StaticArrays, Nested}

// The following also works
// #import "../more-shader-files/reachme.wgsl" as reachme

@group(2) @binding(1)
// TODO: Fix this, I think the bug is in naga_oil.
// var<storage> rts: array<reachme::RtsStruct>;
var<storage> rts: reachme::RtsStruct;

@group(2) @binding(2)
var<storage> a: Scalars;

@group(2) @binding(3)
var<storage> b: VectorsU32;

@group(2) @binding(4)
var<storage> c: VectorsI32;

@group(2) @binding(5)
var<storage> d: VectorsF32;

@group(2) @binding(6)
var<storage> f: MatricesF32;

@group(2) @binding(8)
var<storage> h: StaticArrays;

@group(2) @binding(9)
var<storage> i: Nested;

@group(0) @binding(0)
var color_texture: texture_2d<f32>;
@group(0) @binding(1)
var color_sampler: sampler;

struct Uniforms {
  color_rgb: vec4<f32>,
  scalars: Scalars
}

@group(1) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
  @location(0) position: vec3<f32>,
};

struct VertexOutput {
  @builtin(position) clip_position: vec4<f32>,
  @location(0) tex_coords: vec2<f32>
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    //A fullscreen triangle.
  var out: VertexOutput;
  out.clip_position = vec4(in.position.xyz, 1.0);
  out.tex_coords = in.position.xy * 0.5 + 0.5 * reachme::ONE;
  return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
  let color = textureSample(color_texture, color_sampler, in.tex_coords).rgb;
  return vec4(color * uniforms.color_rgb.rgb, 1.0);
}
