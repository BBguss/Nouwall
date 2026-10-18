// Glass Blobs: soft colour fields with a frosted grain, follows the cursor slightly (nouwall built-in, CC0).
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = (fragCoord - 0.5 * iResolution.xy) / iResolution.y;
    vec2 m = (iMouse.xy - 0.5 * iResolution.xy) / iResolution.y;
    float t = iTime * 0.15;
    vec3 col = vec3(0.04, 0.05, 0.09);
    for (int i = 0; i < 5; i++) {
        float fi = float(i);
        vec2 c = 0.45 * vec2(sin(t * (1.0 + 0.3 * fi) + fi * 2.1), cos(t * (0.8 + 0.2 * fi) + fi * 1.3));
        c = mix(c, m, 0.08);
        float d = length(uv - c);
        vec3 hue = 0.5 + 0.5 * cos(6.2831 * (vec3(0.0, 0.33, 0.67) + fi * 0.13 + t * 0.2));
        col += hue * 0.22 * smoothstep(0.55, 0.0, d);
    }
    float g = fract(sin(dot(fragCoord, vec2(12.9898, 78.233))) * 43758.5453);
    col += (g - 0.5) * 0.02;
    fragColor = vec4(col, 1.0);
}
