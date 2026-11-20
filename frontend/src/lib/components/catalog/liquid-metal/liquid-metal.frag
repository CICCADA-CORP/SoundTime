/*!
 * Steel-blue variation on the coordinate distortion in Motion Core's Liquid Metal.
 * https://github.com/motion-core/motion-core/blob/74ad6e66/packages/motion-core/src/lib/components/liquid-metal/LiquidMetalScene.svelte
 *
 * MIT License, Copyright (c) 2025 Marek Jóźwiak
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */
precision highp float;
varying vec2 vUv;
uniform float uTime;
uniform vec2 uResolution;
uniform vec3 uPointer;

vec2 fold(vec2 p, float frequency, float amplitude, vec2 tide) {
  p.x += amplitude * sin(frequency * p.y + 0.57 + tide.x);
  p.y += amplitude * sin(frequency * p.x + 1.12 + tide.y);
  return p;
}

vec3 blueSteel(float light) {
  vec3 deep = vec3(30.0, 61.0, 87.0);
  vec3 navy = vec3(50.0, 85.0, 113.0);
  vec3 steel = vec3(86.0, 128.0, 159.0);
  vec3 silver = vec3(157.0, 187.0, 211.0);
  vec3 ice = vec3(217.0, 234.0, 245.0);
  vec3 color = mix(deep, navy, clamp(light / 0.22, 0.0, 1.0));
  color = mix(color, steel, clamp((light - 0.22) / 0.26, 0.0, 1.0));
  color = mix(color, silver, clamp((light - 0.48) / 0.20, 0.0, 1.0));
  return mix(color, ice, clamp((light - 0.68) / 0.17, 0.0, 1.0)) / 255.0;
}

void main() {
  // Preserve the composition as the panel changes shape, as object-fit: cover does for the poster.
  float aspect = uResolution.x / uResolution.y;
  vec2 crop = min(vec2(aspect / 0.8, 0.8 / aspect), vec2(1.0));
  vec2 panelUV = vec2(vUv.x, 1.0 - vUv.y);
  vec2 uv = (panelUV - 0.5) * crop + 0.5;
  vec2 p = (uv - 0.5) * vec2(0.8, 1.0);
  // Bend the existing folds around the pointer in screen space, even on wide panels.
  vec2 offset = (panelUV - uPointer.xy) * uResolution / min(uResolution.x, uResolution.y);
  float influence = uPointer.z * exp(-10.0 * dot(offset, offset));
  vec2 tangent = vec2(-offset.y, offset.x);
  p += influence * (0.24 * offset + 0.33 * tangent + vec2(0.016, -0.021));
  float phase = uTime * 0.27;
  vec2 tide = vec2(0.58 * sin(phase), 0.39 * (cos(phase) - 1.0) - 0.37 * sin(phase));
  p = fold(p, 4.0, 0.116, tide);
  p = fold(p, 9.0, 0.075, tide);
  p = fold(p, 17.0, 0.044, tide);
  p = fold(p, 34.0, 0.017, tide);
  p = fold(p, 73.0, 0.004, tide);

  float radius = length((p + vec2(0.018, 0.081)) * vec2(2.14, 1.70));
  radius += 0.07 * sin(8.0*p.x - 4.0*p.y + tide.x) + 0.035 * cos(17.0*p.y - 2.0*p.x + tide.y);
  float edge = radius - 0.84;
  float fiber = 0.60*sin(9.0*p.x + 6.0*p.y + 0.8*tide.x) + 0.28*sin(14.0*p.y - 9.0*p.x) + 0.12*cos(22.0*p.x + 7.0*p.y);

  // Broad brushed planes pick up the sky, narrow fold edges catch a sharper reflection.
  float light = 0.12 + 0.065*fiber + 0.14*exp(-10.0*abs(fiber)) * mix(0.15, 1.0, smoothstep(0.3, 1.1, radius));
  light += 0.11*smoothstep(-0.16, 0.12, edge) + 0.28*smoothstep(0.02, 0.27, edge) * mix(0.55, 1.0, uv.y);
  light += 0.34*exp(-52.0*abs(edge)) * (0.70 + 0.30*sin(10.0*p.y + 4.0*p.x - tide.x));
  light += 0.19*exp(-30.0*abs(edge-0.049)) * (0.50 + 0.50*cos(8.0*p.y - 7.0*p.x));
  light += 0.19*exp(-20.0*abs(edge+0.091)) * (0.50 + 0.50*sin(12.0*p.x - 4.0*p.y));
  light += 0.22*exp(-10.0*abs(edge-0.24)) * (0.50 + 0.50*cos(17.0*p.x - 9.0*p.y));
  float corner = length((p - vec2(0.40,-0.36)) * vec2(3.9,3.0)) - 0.41;
  light += 0.26*exp(-4.0*abs(corner)) * (0.63 + 0.37*cos(14.0*p.x + 7.0*p.y));
  light += 0.10*smoothstep(0.20, 0.49, edge)*uv.y + 0.15*smoothstep(0.55, 0.94, uv.y)*smoothstep(0.5, 1.0, uv.x)*smoothstep(0.74, 1.05, radius);
  light += 0.03*cos(9.0*uv.x + 13.0*uv.y + 2.0*sin(14.0*uv.x - 21.0*uv.y));
  // Catch a moving highlight along the bent metal grain.
  float reflection = dot(offset, vec2(-0.57, -0.82)) + 0.09 * fiber;
  light += influence * (0.10 + 0.07 * fiber + 0.23 * exp(-12.0 * abs(reflection)));

  // Stationary grain avoids flicker as the highlights move.
  vec2 pixel = floor(uv * vec2(880.0, 1100.0));
  float grain = fract(52.9829189 * fract(dot(pixel, vec2(0.06711056, 0.00583715))));
  gl_FragColor = vec4(blueSteel(light + (grain - 0.5) * 0.068), 1.0);
}
