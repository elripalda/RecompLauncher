// RecompLauncher look: Film.
// Warm highlights and cool shadows, a softer roll-off at the top, fine moving grain and a light
// vignette.
// Part of RecompLauncher (GPL-3.0-or-later). https://recomplauncher.com

#include "RL.fxh"

uniform float Tone <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Colour grade";
	ui_tooltip = "Warm highlights, cool shadows.";
> = 0.5;

uniform float Grain <
	ui_type = "slider"; ui_min = 0.0; ui_max = 0.2;
	ui_label = "Grain";
> = 0.05;

uniform float GrainSize <
	ui_type = "slider"; ui_min = 1.0; ui_max = 3.0;
	ui_label = "Grain size";
> = 1.5;

uniform float Vignette <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Vignette";
> = 0.3;

uniform int RL_Frame < source = "framecount"; >;

float3 RL_FilmPS(float4 pos : SV_Position, float2 uv : TEXCOORD) : SV_Target
{
	float3 c = RL_Fetch(uv);
#if RL_SDR
	float luma = dot(c, RL_LUMA);

	// A softer top end and blacks lifted a touch, the way print film never quite reaches black.
	float k = 0.3 * Tone;
	c = c * (1.0 + k) / (1.0 + c * k);
	c = c * (1.0 - 0.035 * Tone) + 0.035 * Tone;

	// Split toning: teal under the mids, amber over them.
	float3 shadows = float3(-0.025, 0.006, 0.03);
	float3 highs = float3(0.035, 0.012, -0.03);
	float hw = smoothstep(0.35, 0.9, luma);
	float sw = 1.0 - smoothstep(0.05, 0.5, luma);
	c += (shadows * sw + highs * hw) * Tone * 1.6;

	// A little less saturation, like print stock.
	c = lerp(dot(c, RL_LUMA).xxx, c, 1.0 - 0.12 * Tone);

	// Vignette: an oval that follows the screen's shape.
	float2 d = (uv - 0.5) * float2(1.0, BUFFER_HEIGHT * BUFFER_RCP_WIDTH) * 1.6;
	float v = saturate(1.0 - dot(d, d) * 0.6);
	c *= lerp(1.0, v * v * (3.0 - 2.0 * v), Vignette);

	// Grain: strongest in the mids, where film shows it most, and new every frame.
	float2 cell = floor(pos.xy / GrainSize);
	float n = RL_Hash(cell + float(RL_Frame % 1024) * 17.13) + RL_Hash(cell * 1.7 + float(RL_Frame % 997) * 3.71) - 1.0;
	float mid = 1.0 - abs(luma * 2.0 - 1.0);
	c += n * Grain * (0.35 + 0.65 * mid);

	c = saturate(c);
#endif
	return c;
}

technique RL_Film <
	ui_label = "RecompLauncher: Film";
	ui_tooltip = "A graded, filmic picture with fine grain.";
>
{
	pass
	{
		VertexShader = RL_VS;
		PixelShader = RL_FilmPS;
	}
}
