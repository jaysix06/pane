-- The LED dot-matrix meter: Pane's signature data display. One WGSL fill
-- shader draws a grid of round dots; columns light up in their category's
-- colour up to how full the drive is, unlit dots stay a dim grid, lit dots
-- bloom, and while scanning a shimmer runs along the fill edge.
--
-- The shader reads 16 parameters (shaderParam0..15), animated with anime so
-- the bands grow smoothly without running the script per frame:
--   0..10  cumulative category boundaries (fraction of the drive, in order)
--   11     reveal (0..1: the fill animation)
--   12     highlighted category (1..11, 0 for none): the others dim
--   13     glow strength (0..1)
--   14     scan phase (animated while scanning)
--   15     scanning (0 or 1)

local theme = require("../theme")

local M = {}

local function colours_wgsl(): string
	local parts = {}
	for _, c in theme.categories do
		local r, g, b = theme.parse(c.color)
		table.insert(parts, string.format("vec3<f32>(%.4f, %.4f, %.4f)", r / 255, g / 255, b / 255))
	end
	return table.concat(parts, ", ")
end

local cache: { [string]: any } = {}

-- A shader for dots `dot` points wide, `gap` apart, on a dark or light theme.
local function shader(dot: number, gap: number, light: boolean)
	local key = `{dot}:{gap}:{light}`
	if cache[key] then
		return cache[key]
	end
	local off = if light then "vec4<f32>(0.23, 0.16, 0.07, 0.10)" else "vec4<f32>(1.0, 0.92, 0.80, 0.075)"
	local src = string.format(
		[[
const DOT: f32 = %.2f;
const GAP: f32 = %.2f;
const LIGHT: f32 = %.1f;
const COLOURS = array<vec3<f32>, 11>(%s);

fn hash(p: vec2<f32>) -> f32 {
	return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn param(i: u32, input: FillInput) -> f32 {
	var ps = input.params;
	return ps[i / 4u][i %% 4u];
}

fn fill(input: FillInput) -> vec4<f32> {
	let pitch = DOT + GAP;
	let cols = max(floor((input.size.x + GAP) / pitch), 1.0);
	let rows = max(floor((input.size.y + GAP) / pitch), 1.0);
	let used_w = cols * pitch - GAP;
	let used_h = rows * pitch - GAP;
	let origin = (input.size - vec2<f32>(used_w, used_h)) * 0.5;
	let q = input.p - origin;
	let cell = floor(q / pitch);
	if (cell.x < 0.0 || cell.y < 0.0 || cell.x >= cols || cell.y >= rows) {
		return vec4<f32>(0.0);
	}
	let centre = cell * pitch + vec2<f32>(DOT * 0.5);
	let d = length(q - centre);
	let r = DOT * 0.5;
	let aa = 0.75 / max(input.scale, 1.0);
	let disc = 1.0 - smoothstep(r - aa, r + aa, d);

	// Which category this column belongs to, and whether it is lit.
	let x = (cell.x + 0.5) / cols;
	let reveal = param(11u, input);
	let used = param(10u, input) * reveal;
	var cat = 10u;
	for (var i = 0u; i < 11u; i = i + 1u) {
		if (x <= param(i, input) * reveal) {
			cat = i;
			break;
		}
	}
	let lit = x <= used;
	let off = %s;
	if (!lit) {
		return vec4<f32>(off.rgb, off.a * disc);
	}

	var colours = COLOURS;
	var colour = colours[cat];
	var level = 1.0;
	let hi = u32(param(12u, input));
	if (hi > 0u && hi - 1u != cat) {
		level = 0.28;
	}
	// A slight per-dot variance, like real LEDs.
	level = level * (0.86 + 0.14 * hash(cell));

	// The scan front: dots near the fill edge shimmer while scanning.
	let scanning = param(15u, input);
	let edge = exp(-abs(x - used) * cols / 2.5);
	let phase = param(14u, input);
	let twinkle = 0.5 + 0.5 * sin(phase * 6.2831 + cell.y * 1.7 + cell.x * 0.35);
	level = level + scanning * edge * (0.6 * twinkle);

	// Bloom: light spills past the dot's edge.
	let glow = param(13u, input) * (1.0 - LIGHT * 0.6);
	let halo = exp(-(d * d) / (r * r * 2.2)) * glow * 0.55;
	let a = clamp(disc * min(level, 1.4) + halo * level, 0.0, 1.0);
	let rgb = colour * min(level, 1.0) + vec3<f32>(edge * scanning * 0.25);
	return vec4<f32>(min(rgb, vec3<f32>(1.0)), a);
}
]],
		dot,
		gap,
		if light then 1 else 0,
		colours_wgsl(),
		off
	)
	local s = ui.shader(src)
	cache[key] = s
	return s
end

export type MeterData = {
	-- Bytes per category, in theme.categories order (missing = 0).
	bytes: { [string]: number },
	total: number,
}

-- The 11 cumulative boundaries (fractions of the drive) for `data`.
function M.boundaries(data: MeterData?): { number }
	local out = table.create(11, 0)
	if not data or data.total <= 0 then
		return out
	end
	-- Every non-empty category gets at least a sliver (~0.8%) so it stays
	-- visible as a column; the extra is taken from the biggest band.
	local MIN = 0.008
	local shares = {}
	local biggest, big_i = 0, 1
	for i, c in theme.categories do
		local s = (data.bytes[c.id] or 0) / data.total
		shares[i] = s
		if s > biggest then
			biggest, big_i = s, i
		end
	end
	for i, s in shares do
		if s > 0 and s < MIN and i ~= big_i then
			shares[big_i] -= MIN - s
			shares[i] = MIN
		end
	end
	local acc = 0
	for i = 1, #theme.categories do
		acc += math.max(shares[i], 0)
		out[i] = math.min(acc, 1)
	end
	return out
end

-- A meter node.
--   data       () -> MeterData?   what to show
--   scanning   () -> boolean
--   highlight  () -> number       category index to keep bright (0: all)
--   glow       () -> number       0..1
--   motion     () -> boolean      animate (false: jump)
--   dot, gap   points; h height; any other layout props pass through.
function M.meter(props: { [string]: any }): Node
	local dot = props.dot or 6
	local gap = props.gap or 3
	local el_ref = ui.ref()
	local scan_anim: any = nil
	-- Only a change of these restarts an animation (progress arrives every
	-- 100 ms with the same flags).
	local scanning = ui.computed(function()
		return props.scanning and props.scanning() or false
	end)
	local moving = ui.computed(function()
		return if props.motion then props.motion() else true
	end)

	local function apply(duration: number)
		local el = el_ref:peek()
		if not el then
			return
		end
		local b = M.boundaries(props.data())
		local params: { [string]: any } = {
			duration = duration,
			ease = "out(4)",
		}
		for i = 1, 11 do
			params[`shaderParam{i - 1}`] = b[i]
		end
		params.shaderParam12 = props.highlight and props.highlight() or 0
		params.shaderParam13 = props.glow and props.glow() or 0.7
		anime.animate(el, params)
	end

	-- Data or highlight changed: ease the bands to their new places.
	ui.effect(function()
		props.data()
		if props.highlight then
			props.highlight()
		end
		if props.glow then
			props.glow()
		end
		-- While scanning, bands follow the live totals quickly; otherwise ease.
		apply(if not moving:peek() then 0 elseif scanning:peek() then 160 else 700)
	end)

	-- Scanning: run the shimmer; stop it when done so an idle meter costs nothing.
	ui.effect(function()
		local is_scanning = scanning:get()
		local is_moving = moving:get()
		local el = el_ref:peek()
		if not el then
			return
		end
		if scan_anim then
			scan_anim:cancel()
			scan_anim = nil
		end
		if is_scanning and is_moving then
			anime.animate(el, { shaderParam15 = 1, duration = 200 })
			scan_anim = anime.animate(el, { shaderParam14 = { 0, 1 }, duration = 1400, loop = true, ease = "linear" })
		else
			anime.animate(el, { shaderParam15 = 0, duration = 300 })
		end
	end)

	local node = table.clone(props)
	node.data = nil
	node.scanning = nil
	node.highlight = nil
	node.glow = nil
	node.motion = nil
	node.dot = nil
	node.gap = nil
	node.ref = el_ref
	node.bg_shader = shader(dot, gap, ui.theme.kind() == "light")
	node.label = props.label or "Storage meter"
	node.role = "img"
	node.on_mount = function(el)
		-- Start empty, then grow in: the reveal is the entrance.
		local start = { duration = 0, shaderParam11 = 0, shaderParam13 = 0.7 }
		anime.animate(el, start)
		apply(0)
		anime.animate(el, { shaderParam11 = 1, duration = if moving:peek() then 1200 else 0, ease = "out(3)" })
	end
	return ui.div(node)
end

return M
