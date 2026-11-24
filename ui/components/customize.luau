-- Customize look: Pane's appearance customizer. Signature colour (presets,
-- a hue slider, or a hex value), glow, theme, corners, density, motion and
-- backdrop. Every change applies live and is saved by Rust (set_setting).
-- M5's Settings → Appearance page reuses `panel()`.

local state = require("../state")
local theme = require("../theme")
local icons = require("./icons")

local M = {}

M.open = ui.state("customize_open", false)

local function set(key: string, value: any)
	ui.invoke("set_setting", { key = key, value = value })
end

local function section(title: string, hint: string?, body: Node): Node
	return ui.column {
		w_full = true,
		gap = 10,
		ui.column {
			gap = 2,
			ui.text { title, weight = 600 },
			ui.show(hint ~= nil, ui.text { hint or "", color = "$text.tertiary", text_size = "$font.size.sm" }),
		},
		body,
	}
end

local function swatch(preset: { id: string, label: string, color: string }): Node
	local function chosen(): boolean
		local a = state.setting("accent")
		return type(a) == "string" and string.upper(a) == string.upper(preset.color)
	end
	return ui.button {
		label = preset.label,
		tooltip = preset.label,
		on_click = function()
			set("accent", preset.color)
		end,
		w = 34,
		h = 34,
		p = 0,
		rounded = "$radius.full",
		bg = "#00000000",
		border = 2,
		border_color = function()
			return if chosen() then preset.color else "#00000000"
		end,
		items = "center",
		justify = "center",
		ui.div {
			w = 24,
			h = 24,
			rounded = "$radius.full",
			bg = preset.color,
			shadow = { x = 0, y = 0, blur = 12, spread = 0, color = theme.alpha(preset.color, 0.55) },
		},
	}
end

local function segmented(key: string, options: { { label: string, value: string } }): Node
	return ui.segmented {
		label = key,
		options = options,
		value = function()
			return state.setting(key)
		end,
		on_change = function(v)
			set(key, v)
		end,
	}
end

-- The hue slider's position, from the current accent.
local hue = ui.state("customize_hue", 40)
local hex_text = ui.state("customize_hex", "")

local function colour_section(): Node
	local presets = {}
	for _, p in theme.presets do
		table.insert(presets, swatch(p))
	end
	return section(
		"Signature colour",
		"Lights up selection, focus, meters and glows.",
		ui.column {
			gap = 12,
			w_full = true,
			ui.row { gap = 6, items = "center", table.unpack(presets) },
			ui.row {
				gap = 12,
				items = "center",
				w_full = true,
				ui.div {
					grow = 1,
					h = 14,
					rounded = "$radius.full",
					gradient = {
						angle = 90,
						stops = {
							theme.from_hue(0),
							theme.from_hue(60),
							theme.from_hue(120),
							theme.from_hue(180),
							theme.from_hue(240),
							theme.from_hue(300),
							theme.from_hue(359),
						},
					},
					ui.slider {
						label = "Hue",
						value = hue,
						min = 0,
						max = 359,
						step = 1,
						w_full = true,
						h = 14,
						py = 0,
						px = 0,
						bg = "#00000000",
						on_change = function(v: number)
							hue:set(v)
							set("accent", theme.from_hue(v))
						end,
					},
				},
				ui.input {
					value = function()
						local a = state.setting("accent")
						return if type(a) == "string" then a else ""
					end,
					label = "Colour (hex)",
					placeholder = "#FFB020",
					font = "$font.mono",
					w = 104,
					h = 32,
					on_commit = function(text: string)
						local t = string.upper(string.gsub(text, "%s", ""))
						if string.sub(t, 1, 1) ~= "#" then
							t = "#" .. t
						end
						if string.match(t, "^#%x%x%x%x%x%x$") then
							set("accent", t)
						end
						hex_text:set(t)
					end,
				},
			},
		}
	)
end

local function glow_section(): Node
	return section(
		"Glow",
		"How much the meters, focus rings and selection bloom.",
		ui.row {
			gap = 12,
			items = "center",
			w_full = true,
			-- A track that brightens toward the accent, the slider over it.
			ui.div {
				grow = 1,
				h = 14,
				rounded = "$radius.full",
				gradient = function()
					local a = theme.accent_for(state.setting("accent"), ui.theme.kind() == "light")
					return { angle = 90, stops = { theme.alpha(a, 0.08), a } }
				end,
				ui.slider {
				label = "Glow intensity",
				w_full = true,
				h = 14,
				px = 0,
				py = 0,
				bg = "#00000000",
				value = function()
					return state.setting("glow") or 70
				end,
				on_change = function(v: number)
					set("glow", math.floor(v + 0.5))
				end,
				min = 0,
				max = 100,
				step = 5,
				grow = 1,
				value_text = function()
					return `{state.setting("glow") or 70}%`
				end,
				},
			},
			ui.text {
				function()
					return `{state.setting("glow") or 70}%`
				end,
				font = "$font.mono",
				w = 44,
				align = "end",
				color = "$text.secondary",
			},
		}
	)
end

-- The customizer's controls (no frame): reused by Settings → Appearance.
function M.panel(): Node
	return ui.column {
		w_full = true,
		gap = 22,
		colour_section(),
		glow_section(),
		section(
			"Theme",
			nil,
			segmented("theme", {
				{ label = "System", value = "system" },
				{ label = "Dark", value = "dark" },
				{ label = "Light", value = "light" },
			})
		),
		ui.row {
			gap = 22,
			w_full = true,
			section(
				"Corners",
				nil,
				segmented("corners", { { label = "Soft", value = "soft" }, { label = "Sharp", value = "sharp" } })
			),
			section(
				"Density",
				nil,
				segmented("density", {
					{ label = "Comfortable", value = "comfortable" },
					{ label = "Compact", value = "compact" },
				})
			),
		},
		section(
			"Motion",
			"System follows Windows' animation effects setting.",
			segmented("animations", {
				{ label = "System", value = "system" },
				{ label = "On", value = "on" },
				{ label = "Off", value = "off" },
			})
		),
		section(
			"Window backdrop",
			"Applies the next time Pane opens.",
			segmented("backdrop", {
				{ label = "Mica", value = "mica" },
				{ label = "Mica Alt", value = "mica_alt" },
				{ label = "Acrylic", value = "acrylic" },
				{ label = "Solid", value = "solid" },
			})
		),
	}
end

-- The customizer as a side sheet.
function M.sheet(): Node
	return ui.modal {
		open = M.open,
		label = "Customize look",
		placement = { x = "end", y = "start", inset = 12 },
		w = 420,
		p = 0,
		bg = "#00000000",
		build = function()
			return ui.column {
				w = 420,
				-- As tall as the window allows (less the inset); the controls
				-- scroll inside. A modal's panel doesn't take a percentage
				-- height, so this reads the window's size.
				-- (read once as it opens: no script runs while resizing).
				h = (function()
					local size = ui.window.size:peek()
					return if size and size.height then math.max(320, size.height - 24) else 600
				end)(),
				p = 22,
				gap = 20,
				bg = "$surface.overlay",
				rounded = "$radius.xl",
				border = 1,
				border_color = "$border.subtle",
				shadow = "$shadow.lg",
				ui.row {
					w_full = true,
					items = "center",
					gap = 10,
					icons.icon("palette", 20, "$accent"),
					ui.text { "Customize look", text_size = "$font.size.lg", weight = 600, grow = 1 },
					ui.button {
						label = "Reset look",
						on_click = function()
							ui.invoke("reset_settings", {
								keys = { "accent", "glow", "theme", "corners", "density", "animations", "backdrop" },
							})
						end,
						px = 10,
						h = 28,
						rounded = "$radius.md",
						bg = "#00000000",
						hover = { bg = "$hover" },
						ui.text { "Reset", color = "$text.secondary", text_size = "$font.size.sm" },
					},
					ui.button {
						label = "Close",
						on_click = ui.set(M.open, false),
						w = 28,
						h = 28,
						p = 0,
						rounded = "$radius.md",
						bg = "#00000000",
						hover = { bg = "$hover" },
						items = "center",
						justify = "center",
						icons.icon("close", 14),
					},
				},
				ui.scroll { w_full = true, grow = 1, gap = 0, M.panel() },
			}
		end,
	}
end

return M
