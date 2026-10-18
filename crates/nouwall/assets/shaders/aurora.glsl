// Aurora: drifting aurora bands over a starry sky (nouwall built-in, CC0).
float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }

float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
               mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}

float fbm(vec2 p) {
    float v = 0.0;
    float a = 0.5;
    for (int i = 0; i < 5; i++) {
        v += a * noise(p);
        p *= 2.0;
        a *= 0.5;
    }
    return v;
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    float t = iTime * 0.05;
    vec3 col = mix(vec3(0.02, 0.03, 0.08), vec3(0.05, 0.02, 0.12), uv.y);
    for (int i = 0; i < 3; i++) {
        float fi = float(i);
        float y = 0.55 + 0.12 * fi + 0.08 * sin(uv.x * 3.0 + t * 6.0 + fi * 1.7)
                + 0.1 * (fbm(vec2(uv.x * 2.0 + t * 2.0 + fi, t + fi)) - 0.5);
        float d = (uv.y - y) * (14.0 - 3.0 * fi);
        float band = exp(-d * d);
        float rays = 0.6 + 0.4 * fbm(vec2(uv.x * 24.0 + fi * 5.0, t * 8.0));
        vec3 tint = mix(vec3(0.1, 0.9, 0.6), vec3(0.5, 0.3, 1.0), fi / 2.0);
        col += tint * band * rays * 0.55;
    }
    float star = step(0.997, hash(floor(fragCoord * 0.5)));
    float twinkle = 0.5 + 0.5 * sin(iTime * 2.0 + hash(floor(fragCoord)) * 30.0);
    col += vec3(star * twinkle) * smoothstep(0.3, 1.0, uv.y);
    fragColor = vec4(col, 1.0);
}
