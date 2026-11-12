// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

// Cubemap to equidistant fisheye remap shader.
//
// Samples a cubemap texture using equidistant fisheye projection and
// writes the result to a 2D output texture.  Each output pixel maps to
// a direction in camera space, which is then used to sample the cubemap.
//
// Uniforms:
//   hfov  — horizontal field of view in radians
//   vfov  — vertical field of view in radians (derived from aspect)
//   yaw   — camera yaw offset from ego heading (radians)
//   pitch — camera pitch (radians, negative = look down)

struct RemapUniform {
    hfov: f32,
    vfov: f32,
    yaw: f32,
    pitch: f32,
};

@group(0) @binding(0) var cubemap: texture_cube<f32>;
@group(0) @binding(1) var cube_sampler: sampler;
@group(0) @binding(2) var<uniform> params: RemapUniform;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Full-screen triangle (3 vertices, no vertex buffer needed).
@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var out: VertexOutput;
    out.position = vec4<f32>(pos[idx], 0.0, 1.0);
    // Map clip coords to [0,1] UV.
    out.uv = pos[idx] * 0.5 + 0.5;
    // Flip Y so that uv.y=0 is top of image.
    out.uv.y = 1.0 - out.uv.y;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Pixel position in normalised image coords centred at (0,0).
    // cy is positive upward so top of image → sky, bottom → road.
    let cx = in.uv.x - 0.5;
    let cy = 0.5 - in.uv.y;

    // Equidistant fisheye projection with independent H/V FOV.
    // Each axis maps [-0.5, 0.5] to [-fov/2, fov/2] radians.
    let angle_x = cx * params.hfov;
    let angle_y = cy * params.vfov;

    // Polar angle from optical axis and azimuth in angular space.
    let theta = sqrt(angle_x * angle_x + angle_y * angle_y);

    // Reject pixels beyond hemisphere (cubemap can only cover ±90°).
    if theta > 1.5707963 {
        return vec4<f32>(0.627, 0.706, 0.784, 1.0); // sky colour
    }

    let phi = atan2(angle_y, angle_x);

    // Direction in camera-local space (z = forward, x = right, y = up).
    let sin_theta = sin(theta);
    let dir_cam = vec3<f32>(
        sin_theta * cos(phi),
        sin_theta * sin(phi),
        cos(theta),
    );

    // Apply camera pitch (rotation around X axis).
    // Negate pitch so that negative pitch = look down.
    let p = -params.pitch;
    let cp = cos(p);
    let sp = sin(p);
    let dir_pitched = vec3<f32>(
        dir_cam.x,
        dir_cam.y * cp - dir_cam.z * sp,
        dir_cam.y * sp + dir_cam.z * cp,
    );

    // Sample the cubemap.
    let colour = textureSample(cubemap, cube_sampler, dir_pitched);
    return colour;
}
