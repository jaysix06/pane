-- Syntax colours for the preview pane's code: keywords, strings, comments,
-- numbers and type-like names, by a few families of languages picked from
-- the file's extension. A line at a time (zgx's textarea `highlight`): the
-- state carried to the next line says a block comment or a long string is
-- still open. Small on purpose: it colours, it doesn't parse.

local M = {}

type Lang = {
	line: { string },
	block: { string }?,
	strings: { string },
	keywords: { [string]: boolean },
	-- Words starting with a capital are types (Rust, TypeScript, C#...).
	caps_types: boolean,
}

local function set(words: string): { [string]: boolean }
	local out = {}
	for w in string.gmatch(words, "%S+") do
		out[w] = true
	end
	return out
end

local C_LIKE = { "/*", "*/" }

local LANGS: { [string]: Lang } = {
	rust = {
		line = { "//" },
		block = C_LIKE,
		strings = { '"' },
		keywords = set(
			"as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while"
		),
		caps_types = true,
	},
	c = {
		line = { "//" },
		block = C_LIKE,
		strings = { '"', "'" },
		keywords = set(
			"auto bool break case catch char class const constexpr continue default delete do double else enum explicit extern false float for friend goto if inline int long namespace new nullptr operator private protected public return short signed sizeof static struct switch template this throw true try typedef typename union unsigned using virtual void volatile while include define ifdef ifndef endif pragma"
		),
		caps_types = true,
	},
	cs = {
		line = { "//" },
		block = C_LIKE,
		strings = { '"', "'" },
		keywords = set(
			"abstract as async await base bool break byte case catch char class const continue default delegate do double else enum event false finally float for foreach get if in int interface internal is long namespace new null object out override private protected public readonly ref return sealed set static string struct switch this throw true try typeof using var virtual void while"
		),
		caps_types = true,
	},
	js = {
		line = { "//" },
		block = C_LIKE,
		strings = { '"', "'", "`" },
		keywords = set(
			"async await break case catch class const continue default delete do else export extends false finally for from function if import in instanceof interface let new null of return static super switch this throw true try type typeof undefined var void while yield enum implements private public readonly"
		),
		caps_types = true,
	},
	go = {
		line = { "//" },
		block = C_LIKE,
		strings = { '"', "'", "`" },
		keywords = set(
			"break case chan const continue default defer else fallthrough false for func go goto if import interface map nil package range return select struct switch true type var"
		),
		caps_types = false,
	},
	python = {
		line = { "#" },
		block = nil,
		strings = { '"', "'" },
		keywords = set(
			"and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return self True try while with yield"
		),
		caps_types = true,
	},
	shell = {
		line = { "#" },
		block = nil,
		strings = { '"', "'" },
		keywords = set(
			"if then else elif fi for while do done case esac in function return local export echo param begin process end foreach switch try catch"
		),
		caps_types = false,
	},
	lua = {
		line = { "--" },
		block = { "--[[", "]]" },
		strings = { '"', "'", "`" },
		keywords = set(
			"and break continue do else elseif end false for function if in local nil not or repeat return then true type until while export"
		),
		caps_types = true,
	},
	sql = {
		line = { "--" },
		block = C_LIKE,
		strings = { "'" },
		keywords = set(
			"select from where and or not insert into values update set delete create table index view drop alter join left right inner outer on group by order having limit as distinct null is in like between case when then else end primary key foreign references SELECT FROM WHERE AND OR NOT INSERT INTO VALUES UPDATE SET DELETE CREATE TABLE INDEX VIEW DROP ALTER JOIN LEFT RIGHT INNER OUTER ON GROUP BY ORDER HAVING LIMIT AS DISTINCT NULL IS IN LIKE BETWEEN CASE WHEN THEN ELSE END PRIMARY KEY FOREIGN REFERENCES"
		),
		caps_types = false,
	},
	-- Settings files: keys and values, comments by "#" or ";".
	config = {
		line = { "#", ";" },
		block = nil,
		strings = { '"', "'" },
		keywords = set("true false null yes no on off"),
		caps_types = false,
	},
	json = {
		line = { "//" },
		block = C_LIKE,
		strings = { '"' },
		keywords = set("true false null"),
		caps_types = false,
	},
}

local BY_EXT: { [string]: string } = {
	rs = "rust",
	c = "c",
	h = "c",
	cc = "c",
	cpp = "c",
	cxx = "c",
	hpp = "c",
	hh = "c",
	java = "c",
	kt = "c",
	swift = "c",
	cs = "cs",
	js = "js",
	mjs = "js",
	cjs = "js",
	jsx = "js",
	ts = "js",
	tsx = "js",
	mts = "js",
	go = "go",
	py = "python",
	pyw = "python",
	sh = "shell",
	bash = "shell",
	zsh = "shell",
	ps1 = "shell",
	psm1 = "shell",
	lua = "lua",
	luau = "lua",
	sql = "sql",
	toml = "config",
	ini = "config",
	cfg = "config",
	conf = "config",
	yaml = "config",
	yml = "config",
	env = "config",
	gitignore = "config",
	json = "json",
	jsonc = "json",
}

-- The language for a file name, or nil (plain text: no colours).
function M.language(name: string): string?
	local lower = string.lower(name)
	local ext = string.match(lower, "%.([^.]+)$") or string.match(lower, "^%.(.+)$")
	return if ext then BY_EXT[ext] else nil
end

local COLORS = {
	keyword = "$code.keyword",
	string = "$code.string",
	comment = "$code.comment",
	number = "$code.number",
	type = "$code.type",
}

-- Spans are built with the end one past the last byte; zgx's are
-- inclusive (as string.find gives them).
local function done(spans: { any }, st: any): ({ any }, any)
	for _, sp in spans do
		sp[2] -= 1
	end
	return spans, st
end

local function starts(line: string, at: number, token: string): boolean
	return string.sub(line, at, at + #token - 1) == token
end

-- The highlighter for language `lang`: (line, state) -> (spans, state).
-- State: 0 nothing open, 1 inside a block comment, or a quote character
-- inside a string that runs on (backquotes, Lua's long strings aside).
function M.for_language(lang: string?): ((string, any) -> ({ any }, any))?
	local L = if lang then LANGS[lang] else nil
	if L == nil then
		return nil
	end
	local l: Lang = L
	return function(line: string, st: any)
		local spans = {}
		local i = 1
		local n = #line
		-- A block comment running on from the line before.
		if st == 1 and l.block then
			local close = string.find(line, l.block[2], 1, true)
			if close == nil then
				return done({ { 1, n + 1, COLORS.comment, italic = true } }, 1)
			end
			local stop = close + #l.block[2]
			table.insert(spans, { 1, stop, COLORS.comment, italic = true })
			i = stop
		elseif type(st) == "string" then
			-- A template string (`...`) running on.
			local close = string.find(line, st, 1, true)
			if close == nil then
				return done({ { 1, n + 1, COLORS.string } }, st)
			end
			table.insert(spans, { 1, close + 1, COLORS.string })
			i = close + 1
		end
		while i <= n do
			local ch = string.sub(line, i, i)
			-- Comments: to the end of the line, or a block.
			if l.block and starts(line, i, l.block[1]) then
				local close = string.find(line, l.block[2], i + #l.block[1], true)
				if close == nil then
					table.insert(spans, { i, n + 1, COLORS.comment, italic = true })
					return done(spans, 1)
				end
				local stop = close + #l.block[2]
				table.insert(spans, { i, stop, COLORS.comment, italic = true })
				i = stop
				continue
			end
			local is_comment = false
			for _, tok in l.line do
				if starts(line, i, tok) then
					is_comment = true
					break
				end
			end
			if is_comment then
				table.insert(spans, { i, n + 1, COLORS.comment, italic = true })
				return done(spans, 0)
			end
			-- Strings, with backslash escapes.
			if table.find(l.strings, ch) then
				local j = i + 1
				while j <= n do
					local c = string.sub(line, j, j)
					if c == "\\" then
						j += 2
						continue
					end
					if c == ch then
						break
					end
					j += 1
				end
				if j > n then
					table.insert(spans, { i, n + 1, COLORS.string })
					-- Only a backquote string runs on to the next line.
					return done(spans, if ch == "`" then ch else 0)
				end
				table.insert(spans, { i, j + 1, COLORS.string })
				i = j + 1
				continue
			end
			-- Numbers (hex, decimals, exponents, underscores, suffixes).
			if string.match(ch, "%d") and (i == 1 or not string.match(string.sub(line, i - 1, i - 1), "[%w_]")) then
				local a, b = string.find(line, "^0[xX][%x_]+", i)
				if not a then
					a, b = string.find(line, "^[%d_]+%.?[%d_]*[eE]?[%+%-]?%d*[%a_]*", i)
				end
				table.insert(spans, { i, (b :: number) + 1, COLORS.number })
				i = (b :: number) + 1
				continue
			end
			-- Words: keywords, then capitalised names as types.
			if string.match(ch, "[%a_]") then
				local _, b = string.find(line, "^[%w_]+", i)
				local word = string.sub(line, i, b :: number)
				if l.keywords[word] then
					table.insert(spans, { i, (b :: number) + 1, COLORS.keyword, weight = 600 })
				elseif l.caps_types and string.match(word, "^%u") and string.match(word, "%l") then
					table.insert(spans, { i, (b :: number) + 1, COLORS.type })
				end
				i = (b :: number) + 1
				continue
			end
			i += 1
		end
		return done(spans, 0)
	end
end

return M
