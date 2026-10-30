// RecompLauncher looks: shared bits.
// Part of RecompLauncher (GPL-3.0-or-later). https://recomplauncher.com

#pragma once

#ifndef BUFFER_COLOR_SPACE
	#define BUFFER_COLOR_SPACE 1
#endif

// The looks are made for ordinary (SDR) pictures. On an HDR picture they leave it alone.
#define RL_SDR (BUFFER_COLOR_SPACE <= 1)

texture RL_BackBufferTex : COLOR;
sampler RL_BackBuffer { Texture = RL_BackBufferTex; };

#define RL_PX float2(BUFFER_RCP_WIDTH, BUFFER_RCP_HEIGHT)
#define RL_SIZE float2(BUFFER_WIDTH, BUFFER_HEIGHT)

static const float3 RL_LUMA = float3(0.2126, 0.7152, 0.0722);

void RL_VS(in uint id : SV_VertexID, out float4 pos : SV_Position, out float2 uv : TEXCOORD)
{
	uv.x = (id == 2) ? 2.0 : 0.0;
	uv.y = (id == 1) ? 2.0 : 0.0;
	pos = float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
}

float3 RL_Fetch(float2 uv)
{
	return tex2Dlod(RL_BackBuffer, float4(uv, 0.0, 0.0)).rgb;
}

// A 2.2 curve: close to sRGB, and the two undo each other exactly.
float3 RL_ToLinear(float3 c) { return pow(max(c, 0.0), 2.2); }
float3 RL_ToGamma(float3 c) { return pow(max(c, 0.0), 1.0 / 2.2); }

// A soft knee instead of a hard clip, for looks that brighten.
float3 RL_SoftClip(float3 c)
{
	float3 over = max(c - 0.8, 0.0);
	return min(c, 0.8) + over / (1.0 + over * 5.0);
}

float RL_Hash(float2 p)
{
	float3 p3 = frac(float3(p.xyx) * 0.1031);
	p3 += dot(p3, p3.yzx + 33.33);
	return frac((p3.x + p3.y) * p3.z);
}
