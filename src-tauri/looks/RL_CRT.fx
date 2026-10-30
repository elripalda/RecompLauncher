// RecompLauncher look: CRT.
// The picture as a tube TV drew it: the game's lines become glowing scanlines that widen when
// they're bright, behind a phosphor mask, with a little bloom. The same file makes both the curved
// living-room TV and the flat studio monitor; the launcher picks the settings.
//
// RL_LINES is how many lines the console sent to the TV (240 for most 64-bit era games, 480 for the
// generation after). The launcher sets it from the game's console.
// Part of RecompLauncher (GPL-3.0-or-later). https://recomplauncher.com

#include "RL.fxh"

#ifndef RL_LINES
	#define RL_LINES 240
#endif

// At least two screen pixels per line, or there's nothing to draw scanlines with.
#if RL_LINES * 2 > BUFFER_HEIGHT
	#define RL_H (BUFFER_HEIGHT / 2)
#else
	#define RL_H RL_LINES
#endif
#define RL_W (RL_H * BUFFER_WIDTH / BUFFER_HEIGHT)

uniform float Curvature <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Curve";
> = 0.5;

uniform float Scanlines <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Scanlines";
> = 0.75;

uniform float Beam <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Beam width";
	ui_tooltip = "Thin lines like a studio monitor, or soft wide ones like a TV.";
> = 0.6;

uniform float Mask <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Phosphor mask";
> = 0.45;

uniform int MaskType <
	ui_type = "combo";
	ui_items = "Aperture grille\0Slot mask\0Shadow mask\0";
	ui_label = "Mask type";
> = 1;

uniform float Glow <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Glow";
> = 0.35;

uniform float Sharpness <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Sharpness";
> = 0.35;

uniform float Vignette <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Vignette";
> = 0.25;

uniform float Corner <
	ui_type = "slider"; ui_min = 0.0; ui_max = 1.0;
	ui_label = "Rounded corners";
> = 0.4;

uniform float Brightness <
	ui_type = "slider"; ui_min = 0.5; ui_max = 1.5;
	ui_label = "Brightness";
> = 1.0;

// The game's picture at the console's own size, in linear light.
texture RL_CRTLowTex { Width = RL_W; Height = RL_H; Format = RGBA16F; };
sampler RL_CRTLow { Texture = RL_CRTLowTex; MagFilter = POINT; MinFilter = POINT; MipFilter = POINT; AddressU = CLAMP; AddressV = CLAMP; };
sampler RL_CRTLowLin { Texture = RL_CRTLowTex; AddressU = CLAMP; AddressV = CLAMP; };

// Bloom, at half that size.
texture RL_CRTGlowATex { Width = RL_W / 2; Height = RL_H / 2; Format = RGBA16F; };
texture RL_CRTGlowBTex { Width = RL_W / 2; Height = RL_H / 2; Format = RGBA16F; };
sampler RL_CRTGlowA { Texture = RL_CRTGlowATex; AddressU = CLAMP; AddressV = CLAMP; };
sampler RL_CRTGlowB { Texture = RL_CRTGlowBTex; AddressU = CLAMP; AddressV = CLAMP; };

static const float2 RL_LOW = float2(RL_W, RL_H);
#if BUFFER_HEIGHT / RL_H > 6
	#define RL_TAPS 6
#else
	#define RL_TAPS (BUFFER_HEIGHT / RL_H)
#endif
// The mask's stripes: one pixel each up to 1080p, wider on bigger screens.
#if (BUFFER_HEIGHT + 540) / 1080 > 1
	#define RL_MASK_PX float((BUFFER_HEIGHT + 540) / 1080)
#else
	#define RL_MASK_PX 1.0
#endif

// 1. Shrink the picture to the console's size, averaging every screen pixel under each new one.
float4 RL_CRTDownPS(float4 pos : SV_Position, float2 uv : TEXCOORD) : SV_Target
{
	float3 sum = 0.0;
	[unroll] for (int y = 0; y < RL_TAPS; y++)
	{
		[unroll] for (int x = 0; x < RL_TAPS; x++)
		{
			float2 o = (float2(x, y) + 0.5) / RL_TAPS - 0.5;
			sum += RL_ToLinear(RL_Fetch(uv + o / RL_LOW));
		}
	}
	return float4(sum / (RL_TAPS * RL_TAPS), 1.0);
}

float3 RL_Blur(sampler s, float2 uv, float2 step)
{
	static const float w[5] = { 0.2270270, 0.1945946, 0.1216216, 0.0540541, 0.0162162 };
	float3 c = tex2Dlod(s, float4(uv, 0.0, 0.0)).rgb * w[0];
	[unroll] for (int i = 1; i < 5; i++)
	{
		c += tex2Dlod(s, float4(uv + step * i, 0.0, 0.0)).rgb * w[i];
		c += tex2Dlod(s, float4(uv - step * i, 0.0, 0.0)).rgb * w[i];
	}
	return c;
}

// 2 and 3. Bloom: a wide blur of the small picture, across and then down.
float4 RL_CRTGlowHPS(float4 pos : SV_Position, float2 uv : TEXCOORD) : SV_Target
{
	return float4(RL_Blur(RL_CRTLowLin, uv, float2(2.0 / RL_W, 0.0)), 1.0);
}
float4 RL_CRTGlowVPS(float4 pos : SV_Position, float2 uv : TEXCOORD) : SV_Target
{
	return float4(RL_Blur(RL_CRTGlowA, uv, float2(0.0, 2.0 / RL_H)), 1.0);
}

float2 RL_Warp(float2 uv)
{
	float2 c = uv * 2.0 - 1.0;
	float2 k = Curvature * float2(1.0 / 24.0, 1.0 / 16.0);
	c *= 1.0 + c.yx * c.yx * k;
	return c * 0.5 + 0.5;
}

float3 RL_Texel(int2 at)
{
	at = clamp(at, int2(0, 0), int2(RL_W - 1, RL_H - 1));
	return tex2Dlod(RL_CRTLow, float4((float2(at) + 0.5) / RL_LOW, 0.0, 0.0)).rgb;
}

// One of the console's lines at a point across it, with the soft edges an analogue signal has.
float3 RL_Line(int row, float x)
{
	float px = x - 0.5;
	int i = int(floor(px));
	float f = px - floor(px);
	float hard = lerp(1.6, 5.0, Sharpness);
	float3 c = 0.0;
	float total = 0.0;
	[unroll] for (int k = -1; k <= 2; k++)
	{
		float d = float(k) - f;
		float w = exp(-hard * d * d);
		c += RL_Texel(int2(i + k, row)) * w;
		total += w;
	}
	return c / total;
}

// How much of a line lights a point d lines from its centre. Bright lines are wider, and the
// total light across a line stays the same, so the picture keeps its brightness.
float3 RL_BeamOf(float3 c, float d)
{
	float lum = sqrt(max(dot(c, RL_LUMA), 0.0));
	float thin = lerp(0.14, 0.22, Beam), wide = lerp(0.28, 0.42, Beam);
	float s = lerp(thin, wide, saturate(lum));
	float w = exp(-0.5 * d * d / (s * s)) / (s * 2.5066283);
	return c * w;
}

float3 RL_MaskAt(float2 px)
{
	int x = int(floor(px.x / RL_MASK_PX));
	int y = int(floor(px.y / RL_MASK_PX));
	float dark = 1.0 - 0.7 * Mask;
	float3 m = dark.xxx;
	int sub;
	if (MaskType == 2)
		sub = (x + (y % 2) * 3) % 6 / 2; // shadow mask: dots in staggered rows
	else
		sub = x % 3;
	if (sub == 0) m.r = 1.0; else if (sub == 1) m.g = 1.0; else m.b = 1.0;
	if (MaskType == 1)
	{
		// Slot mask: the stripes break into slots, offset on every other triad.
		int triad = x / 3;
		if ((y + (triad % 2) * 2) % 4 == 0)
			m *= dark;
	}
	// Make up for the light the mask takes away, most of the way.
	float kept = (1.0 + 2.0 * dark) / 3.0;
	if (MaskType == 1)
		kept *= (3.0 + dark) / 4.0;
	return m * lerp(1.0, 1.0 / kept, 0.8);
}

// 4. The tube itself.
float3 RL_CRTPS(float4 pos : SV_Position, float2 uv : TEXCOORD) : SV_Target
{
	float2 w = RL_Warp(uv);

	// Outside the tube, and its rounded corners.
	float aspect = BUFFER_WIDTH * BUFFER_RCP_HEIGHT;
	float2 q = abs(w - 0.5) * 2.0 * float2(aspect, 1.0);
	float r = 0.12 * Corner;
	float2 lim = float2(aspect, 1.0) - r;
	float edge = length(max(q - lim, 0.0)) - r;
	float inside = 1.0 - smoothstep(-0.004, 0.004, edge);
	if (inside <= 0.0)
		return 0.0;

	float2 p = w * RL_LOW;
	float y = p.y - 0.5;
	int row = int(floor(y));
	float f = y - floor(y);

	float3 a = RL_Line(row, p.x);
	float3 b = RL_Line(row + 1, p.x);
	float3 flat = lerp(a, b, f);
	float3 scan = RL_BeamOf(a, f) + RL_BeamOf(b, 1.0 - f);
	float3 c = lerp(flat, scan, Scanlines);

	if (Mask > 0.0)
		c *= RL_MaskAt(pos.xy);

	float3 glow = tex2Dlod(RL_CRTGlowB, float4(w, 0.0, 0.0)).rgb;
	c = c * (1.0 - 0.2 * Glow) + glow * Glow * 0.6;

	float2 v = (w - 0.5) * float2(aspect, 1.0) * 1.3;
	c *= lerp(1.0, saturate(1.0 - dot(v, v) * 0.5), Vignette);

	c *= Brightness * inside;
	return RL_ToGamma(RL_SoftClip(c));
}

float3 RL_CRTPassPS(float4 pos : SV_Position, float2 uv : TEXCOORD) : SV_Target
{
	return RL_Fetch(uv);
}

technique RL_CRT <
	ui_label = "RecompLauncher: CRT";
	ui_tooltip = "The game as a tube TV drew it.";
>
{
#if RL_SDR
	pass Down
	{
		VertexShader = RL_VS;
		PixelShader = RL_CRTDownPS;
		RenderTarget = RL_CRTLowTex;
	}
	pass GlowH
	{
		VertexShader = RL_VS;
		PixelShader = RL_CRTGlowHPS;
		RenderTarget = RL_CRTGlowATex;
	}
	pass GlowV
	{
		VertexShader = RL_VS;
		PixelShader = RL_CRTGlowVPS;
		RenderTarget = RL_CRTGlowBTex;
	}
	pass Tube
	{
		VertexShader = RL_VS;
		PixelShader = RL_CRTPS;
	}
#else
	pass
	{
		VertexShader = RL_VS;
		PixelShader = RL_CRTPassPS;
	}
#endif
}
