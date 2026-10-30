// RecompLauncher look: Sharp.
// Contrast-adaptive sharpening, after AMD's FidelityFX CAS: edges that are already strong get less,
// so it brings out texture detail without halos. Good on games drawn at a higher resolution than the
// console's own, where the picture can look soft.
// Part of RecompLauncher (GPL-3.0-or-later). https://recomplauncher.com

#include "RL.fxh"

uniform float Sharpness <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Sharpness";
> = 0.6;

uniform float Detail <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Fine detail";
	ui_tooltip = "Higher picks out smaller detail, and grain with it.";
> = 0.3;

float3 RL_SharpPS(float4 pos : SV_Position, float2 uv : TEXCOORD) : SV_Target
{
	float3 e = RL_Fetch(uv);
#if RL_SDR
	float2 px = RL_PX;
	float3 a = RL_Fetch(uv + float2(-px.x, -px.y));
	float3 b = RL_Fetch(uv + float2(0.0, -px.y));
	float3 c = RL_Fetch(uv + float2(px.x, -px.y));
	float3 d = RL_Fetch(uv + float2(-px.x, 0.0));
	float3 f = RL_Fetch(uv + float2(px.x, 0.0));
	float3 g = RL_Fetch(uv + float2(-px.x, px.y));
	float3 h = RL_Fetch(uv + float2(0.0, px.y));
	float3 i = RL_Fetch(uv + float2(px.x, px.y));

	// How much room there is before the cross clips, from the darkest and brightest neighbours.
	float3 mn = min(min(min(d, e), min(f, b)), h);
	float3 mn2 = min(mn, min(min(a, c), min(g, i)));
	mn += mn2;
	float3 mx = max(max(max(d, e), max(f, b)), h);
	float3 mx2 = max(mx, max(max(a, c), max(g, i)));
	mx += mx2;

	float3 amp = saturate(min(mn, 2.0 - mx) / max(mx, 1e-5));
	amp = rsqrt(max(amp, 1e-5));

	float peak = lerp(8.0, 5.0, Detail);
	float3 w = -1.0 / (amp * peak);
	float3 sharp = saturate(((b + d + f + h) * w + e) / (4.0 * w + 1.0));
	e = lerp(e, sharp, Sharpness);
#endif
	return e;
}

technique RL_Sharp <
	ui_label = "RecompLauncher: Sharp";
	ui_tooltip = "Brings out texture detail without halos.";
>
{
	pass
	{
		VertexShader = RL_VS;
		PixelShader = RL_SharpPS;
	}
}
