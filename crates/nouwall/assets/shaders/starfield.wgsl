// Starfield: layered stars drifting towards the viewer (nouwall built-in, CC0).
// WGSL wallpapers define main_image and read uniforms from `u`.
fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn main_image(frag_coord: vec2<f32>) -> vec4<f32> {
    let res = u.resolution.xy;
    let uv = (frag_coord - 0.5 * res) / res.y;
    var col = vec3<f32>(0.01, 0.01, 0.03);
    for (var layer = 0; layer < 4; layer++) {
        let fl = f32(layer);
        let depth = fract(fl * 0.25 + u.time * 0.03);
        let scale = mix(20.0, 0.5, depth);
        let p = uv * scale + vec2<f32>(fl * 13.7, fl * 7.1);
        let cell = floor(p);
        let f = fract(p) - vec2<f32>(0.5, 0.5);
        let h = hash2(cell);
        let offs = vec2<f32>(hash2(cell + vec2<f32>(1.3, 1.3)), hash2(cell + vec2<f32>(7.9, 7.9))) - vec2<f32>(0.5, 0.5);
        let d = length(f - offs * 0.7);
        let size = mix(0.02, 0.08, h);
        let star = smoothstep(size, 0.0, d) * smoothstep(0.0, 0.3, depth) * smoothstep(1.0, 0.8, depth);
        col += star * mix(vec3<f32>(0.6, 0.7, 1.0), vec3<f32>(1.0, 0.85, 0.7), h);
    }
    return vec4<f32>(col, 1.0);
}
