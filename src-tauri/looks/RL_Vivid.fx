// RecompLauncher look: Vivid.
// Lifts dull colour more than colour that's already strong, deepens the blacks a touch and adds a
// gentle S-curve. Skin and skies stay believable.
// Part of RecompLauncher (GPL-3.0-or-later). https://recomplauncher.com

#include "RL.fxh"

uniform float Vibrance <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Colour";
> = 0.35;

uniform float Contrast <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Contrast";
> = 0.2;

uniform float Blacks <
	ui_type = "slider"; ui_min = 0.0; ui_max = 0.1;
	ui_label = "Deeper blacks";
> = 0.02;

float3 RL_VividPS(float4 pos : SV_Position, float2 uv : TEXCOORD) : SV_Target
{
	float3 c = RL_Fetch(uv);
#if RL_SDR
	// Blacks first, so the curve works on the new range.
	c = saturate((c - Blacks) / (1.0 - Blacks));

	// Contrast: a smooth S-curve that keeps pure black and white where they are.
	float3 s = c * c * (3.0 - 2.0 * c);
	c = lerp(c, s, Contrast);

	// Vibrance: the less saturated a pixel is, the more it gains.
	float luma = dot(c, RL_LUMA);
	float sat = max(max(c.r, c.g), c.b) - min(min(c.r, c.g), c.b);
	float gain = 1.0 + Vibrance * (1.0 - sat) * 1.4;
	c = saturate(lerp(luma.xxx, c, gain));
#endif
	return c;
}

technique RL_Vivid <
	ui_label = "RecompLauncher: Vivid";
	ui_tooltip = "Richer colour and a little more contrast.";
>
{
	pass
	{
		VertexShader = RL_VS;
		PixelShader = RL_VividPS;
	}
}
