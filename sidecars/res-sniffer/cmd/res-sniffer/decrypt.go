package main

import (
	"encoding/binary"
	"strconv"
)

type RandCtx64 struct {
	RandCnt uint64
	Seed    [256]uint64
	MM      [256]uint64
	AA      uint64
	BB      uint64
	CC      uint64
}

func CreateISAacInst(encKey uint64) *RandCtx64 {
	ctx := &RandCtx64{
		RandCnt: 255,
		AA:      0,
		BB:      0,
		CC:      0,
	}
	rand64Init(ctx, encKey)
	return ctx
}

func (ctx *RandCtx64) ISAacRandom() uint64 {
	result := ctx.Seed[ctx.RandCnt]
	if ctx.RandCnt == 0 {
		ctx.isAAC64()
		ctx.RandCnt = 255
	} else {
		ctx.RandCnt--
	}
	return result
}

func rand64Init(ctx *RandCtx64, encKey uint64) {
	const golden = uint64(0x9e3779b97f4a7c13)
	a, b, c, d := golden, golden, golden, golden
	e, f, g, h := golden, golden, golden, golden

	ctx.Seed[0] = encKey
	for i := 1; i < 256; i++ {
		ctx.Seed[i] = 0
	}

	for i := 0; i < 4; i++ {
		mix(&a, &b, &c, &d, &e, &f, &g, &h)
	}

	for i := 0; i < 256; i += 8 {
		a += ctx.Seed[i]
		b += ctx.Seed[i+1]
		c += ctx.Seed[i+2]
		d += ctx.Seed[i+3]
		e += ctx.Seed[i+4]
		f += ctx.Seed[i+5]
		g += ctx.Seed[i+6]
		h += ctx.Seed[i+7]
		mix(&a, &b, &c, &d, &e, &f, &g, &h)
		ctx.MM[i] = a
		ctx.MM[i+1] = b
		ctx.MM[i+2] = c
		ctx.MM[i+3] = d
		ctx.MM[i+4] = e
		ctx.MM[i+5] = f
		ctx.MM[i+6] = g
		ctx.MM[i+7] = h
	}

	for i := 0; i < 256; i += 8 {
		a += ctx.MM[i]
		b += ctx.MM[i+1]
		c += ctx.MM[i+2]
		d += ctx.MM[i+3]
		e += ctx.MM[i+4]
		f += ctx.MM[i+5]
		g += ctx.MM[i+6]
		h += ctx.MM[i+7]
		mix(&a, &b, &c, &d, &e, &f, &g, &h)
		ctx.MM[i] = a
		ctx.MM[i+1] = b
		ctx.MM[i+2] = c
		ctx.MM[i+3] = d
		ctx.MM[i+4] = e
		ctx.MM[i+5] = f
		ctx.MM[i+6] = g
		ctx.MM[i+7] = h
	}

	ctx.isAAC64()
	ctx.RandCnt = 255
}

func mix(a, b, c, d, e, f, g, h *uint64) {
	*a ^= *b << 11; *d += *a; *b += *c
	*b ^= *c >> 2;  *e += *b; *c += *d
	*c ^= *d << 8;  *f += *c; *d += *e
	*d ^= *e >> 16; *g += *d; *e += *f
	*e ^= *f << 10; *h += *e; *f += *g
	*f ^= *g >> 4;  *a += *f; *g += *h
	*g ^= *h << 8;  *b += *g; *h += *a
	*h ^= *a >> 9;  *c += *h; *a += *b
}

func (ctx *RandCtx64) isAAC64() {
	a := ctx.AA
	b := ctx.BB + ctx.CC
	c := ctx.CC

	for i := 0; i < 256; i += 4 {
		ctx.stepISAAC64(&a, &b, &c, i, func(x uint64) uint64 { return ^(x ^ (x << 21)) })
		ctx.stepISAAC64(&a, &b, &c, i+1, func(x uint64) uint64 { return x ^ (x >> 5) })
		ctx.stepISAAC64(&a, &b, &c, i+2, func(x uint64) uint64 { return x ^ (x << 12) })
		ctx.stepISAAC64(&a, &b, &c, i+3, func(x uint64) uint64 { return x ^ (x >> 33) })
	}

	ctx.BB = b
	ctx.AA = a
	ctx.CC = c
}

func (ctx *RandCtx64) stepISAAC64(a, b, c *uint64, i int, fn func(uint64) uint64) {
	x := ctx.MM[i]
	*a = fn(*a) + ctx.MM[(i+128)%256]
	y := ctx.MM[(x>>3)%256] + *a + *b
	ctx.MM[i] = y
	*b = ctx.MM[(y>>11)%256] + x
	ctx.Seed[i] = *b
}

func DecryptWeChatVideo(buf []byte, decodeKeyStr string) []byte {
	if len(buf) == 0 || decodeKeyStr == "" {
		return buf
	}
	key, err := strconv.ParseUint(decodeKeyStr, 10, 64)
	if err != nil {
		return buf
	}
	isaac := CreateISAacInst(key)
	xorLength := 131072
	if len(buf) < xorLength {
		xorLength = len(buf)
	}
	out := make([]byte, len(buf))
	copy(out, buf)

	for i := 0; i < xorLength; i += 8 {
		rnd := isaac.ISAacRandom()
		var block [8]byte
		binary.LittleEndian.PutUint64(block[:], rnd)
		for j := 0; j < 8 && i+j < xorLength; j++ {
			out[i+j] ^= block[j]
		}
	}
	return out
}
