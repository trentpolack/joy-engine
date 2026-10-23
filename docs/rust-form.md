# FORM Language and Runtime
FORM is Joy Engine's procedural geometry language. The `joy-form` crate (`crates/joy-form/`, re-exported as `joy_engine::form`) parses `.form` scripts, reads `.forminst` instances, and evaluates scripts into plain geometry records. It is renderer-free (the content layer may not use rendering): no GPU, window, worker, timer, or host handle is involved, evaluation never reads files, and it builds identically for native and `wasm32-unknown-unknown` targets. Joy Engine's renderer consumes its output.

This guide is the language reference and the runtime contract. FORM EDITOR's Script Reference dialog, completion, and hover text come from the same language catalog (`BUILTINS` and `FORM_KEYWORDS` in `crates/joy-form/src/language_information.rs`).

## Scripts and Instances
A `.form` file is a script: the recipe. It declares parameters, builds geometry values, and outputs them; it is the only thing evaluation runs. A `.forminst` file is a FORM instance: one configured use of a script, holding the script (by reference or packed), parameter overrides, an optional material palette, and named variants.

```json
{
  "version": 2,
  "name": "cottage",
  "form": "building.form",
  "overrides": {"width": 8, "floors": 1, "roof_shape": 1},
  "variants": [{"name": "Tall", "overrides": {"floors": 3}}]
}
```

- `version` is `2` (`FORM_INSTANCE_VERSION`). `name` is at most 120 characters.
- Exactly one of `form` or `source` names the script. `form` is a relative `.form` path with forward slashes, resolved from the instance's folder and never leaving the loader's root, so editing the script updates every instance that refers to it. `source` packs the script text (at most 200,000 characters) for sharing one self-contained file; a packed script's `use` paths resolve from the instance's folder.
- `overrides` maps parameter names to numbers, vectors, colors, true/false, text, or serialized mesh and material resources. Parameters it leaves out keep their script defaults.
- `materials` is an optional palette of complete material records (as `default_material()` writes them), validated like any FORM palette: Default first, at most 128 materials, and embedded textures within the texture budgets.
- `variants` is an optional list of at most 16 override sets with trimmed names of 1–64 characters, unique ignoring case. A variant stores explicit overrides, never frozen computed defaults.
- A document is at most 64 MB (`DOCUMENT_FILE_BYTES`), which leaves room for base64 texture data.

Overrides layer in order: the instance's `overrides`, then the chosen variant's, then whatever the host supplies (a level component's `parameters`, for example). `FormInstance::read` and `read_value` validate a document once, `to_json` and `serialize` write it in stable key order (`serialize` refuses output that would not reopen), `script_path(instance_path)` resolves a reference, `overrides_for(variant)` layers a variant, and `parse_script(instance_path, loader)` parses the referenced or packed script with its modules. `FormInstance::packed` and `referencing` construct new documents, and `read_overrides` and `read_variants` validate those fields alone. Failures are `InstanceError`s with stable messages.

`.forminst` replaces the version 1 `.formlab` document, which always packed its script. The engine reads only version 2. FORM EDITOR still opens a `.formlab` file as a packed instance and saves it as `.forminst`; its script must use the current language (see [Migrating From FORM v1](#migrating-from-form-v1)). Levels, Joy Editor, and `FormAssetLibrary` accept `.form` and `.forminst` assets.

## Language Overview
```text
project meta(name="Wave study", info="SURFACE STUDY / 001")

param height = 2.4 meta(min=0, max=5, step=0.1)
param frequency = 0.55 meta(min=0.1, max=1.5, step=0.05)
param tint = rgba(0.93, 0.42, 0.22, 1)
param ridges = true

# ---- Surface.
let terrain = color(grid(64, 64, 24, 24), tint)
wrangle points in terrain as point {
  let wave = sin(point.position.x*frequency)*cos(point.position.z*frequency)
  if ridges AND wave > 0 {
    wave = wave*1.5
  }
  point.position += vector(0, wave*height, 0)
}
output("Terrain", terrain)
```

Geometry is Y-up; lengths use script units (meters for game assets by convention) and angles use radians. `#` and `//` start line comments. Semicolons are optional statement separators. Names are ASCII letters, digits, and underscores, starting with a letter or underscore, and are case-sensitive. Quoted text uses double quotes with JSON escapes (`\n`, `\"`, `\u00e9`) and stays on one line. A statement is a declaration, an assignment, or a function call; a bare value is an error.

These words belong to the grammar and cannot name variables, parameters, functions, enums, members, or modules (they remain valid as record keys and attribute names):
<!-- form-keywords:start -->
`project`, `param`, `meta`, `let`, `const`, `if`, `else`, `match`, `case`, `for`, `foreach`, `as`, `wrangle`, `in`, `fn`, `return`, `ref`, `enum`, `use`, `AND`, `OR`, `NOT`, `true`, `false`, `null`
<!-- form-keywords:end -->

The attribute domains `points`, `faces`, `corners`, and `instances` follow `wrangle` and appear as text in attribute functions. The constants `pi`, `tau`, and `e` are read-only.

### Values and Types
| Type | Written | Notes |
| --- | --- | --- |
| Number | `4`, `0.25`, `.5`, `1e-3` | Finite 64-bit floating point; an expression that produces infinity or NaN is an error. |
| True/false | `true`, `false`, or a comparison | Required by conditions, `AND`, `OR`, and `NOT`. `number(flag)` converts to 1 or 0. |
| Text | `"Gate"` | Compared with `==` and `!=`; there is no text arithmetic or concatenation. |
| Vector | `vector(x, y, z)` | Components `.x`, `.y`, `.z` or `v[0]`–`v[2]`. |
| Color | `rgba(r, g, b, a)` | Linear RGBA; RGB may exceed 1 for HDR glow, and alpha is opacity. Components `.r`, `.g`, `.b`, `.a`. |
| Record | `{kind: "tower", height: 40}` | Immutable fields read with `.name` or `record["name"]`. |
| List | `[400, 900, 1600]` | Immutable items read with `list[index]`. |
| Null | `null` | An empty record value; also clears a face material. |
| Enum member | `Kind.Tower` | A member of a script-declared `enum`. |
| Mesh | `meshBox(…)`, `meshSphere(…)`, a mesh parameter | An immutable resource; convert with `geometry(mesh)` to edit it. |
| Material | `materialPbr(…)`, a material parameter | An immutable PBR material resource. |
| Geometry | `geometry()`, `box(…)`, `grid(…)`, … | Editable points, faces, attributes, and face materials. |
| Instance collection | `instances(source, sites)` | A source geometry placed at every site point. |

Arithmetic (`+ - * / % ^`) works on numbers, and `+ - * /` work component-wise on vectors and colors, with a number broadcasting to every component; both sides of a vector or color operation need the same component count. `^` is exponentiation and associates to the right. Unary `-` negates numbers, vectors, and colors.

| Precedence (tightest first) | Operators |
| --- | --- |
| Postfix | `.member`, `[index]`, `alias.function(…)` |
| Exponent | `^` (so `-2^2` is `-4`) |
| Sign | unary `-`, `+` |
| Multiplicative | `*`, `/`, `%` |
| Additive | `+`, `-` |
| Comparison | `==`, `!=`, `<`, `>`, `<=`, `>=` |
| Negation | `NOT` |
| Conjunction | `AND` |
| Disjunction | `OR` |

### Booleans and Conditions
Comparisons produce `true` or `false`. `if`, `else if`, `AND`, `OR`, and `NOT` require true/false operands; a number is an error (`Conditions require true or false; compare the number explicitly, such as 'count > 0'.`). `AND` skips its right side when the left is false and `OR` when the left is true, so `i > 0 AND list[i - 1] == 0` never reads past the start of `list`. `NOT` binds looser than comparisons: `NOT a < b` negates `a < b`. Logical operators are written in capitals.

Arithmetic on true/false is an error. Where a flag must become a number, write `number(flag)`: `mix(cold, warm, number(lit))`. Comparisons do not chain; `a < b < c` compares a true/false with a number and fails, so write `a < b AND b < c`.

### Comparisons and Equality
`<`, `>`, `<=`, and `>=` compare numbers. `==` and `!=` (and `match`) compare numbers, true/false values, text, enum members of the same enum, an enum member with text (a member equals its own name), and `null` with records (only `null` equals `null`). Comparing values of different kinds is an authoring mistake and fails rather than reporting "unequal"; vectors and colors compare by component (`a.x == b.x`).

### Text and Enums
Write enums on multiple lines with PascalCase members:

```text
enum Kind {
  Tower,
  Block,
  Hall
}
```

Text compares exactly and case-sensitively. An enum declares named choices at the top level of a script or module (1–32 members); read a member as `Kind.Tower`. Members compare with `==`, select record fields (`heights[kind]`), and store as their name when they become attributes or record fields, so `Kind.Tower == "Tower"` is true and consumers read plain text. Members of two different enums never compare, even when their names match. An enum-typed parameter (`param kind = Kind.Tower`) becomes a choice control; see [Parameters](#parameters).

### Records and Lists
`{name: value, "quoted key": value}` builds a record and `[a, b, c]` a list. Fields and items hold numbers, true/false, text, vectors, colors, enum members (stored by name), records, lists, and `null`; geometry, instance collections, and resources cannot be stored. Keys are identifiers or quoted text of 1–64 characters, unique within the record; `__proto__`, `prototype`, and `constructor` are reserved because records can reach JavaScript glTF consumers. Fields evaluate and keep their authored order.

Read fields with `record.name`, `record["name"]`, or `record[member]` with an enum member, and items with `list[index]` (a whole number from 0 to the last index; anything else is an error). A stored list of three or four numbers reads back as a vector or color. `itemCount(value)` counts a list's items, a record's fields, or a vector's or color's components. Records and lists are values: there is no field or item assignment, so build a new record instead.

### Constants, Variables, and Assignment
`let name = value` declares a variable in the current block: the script's top level, or the body of an `if` branch, `match` case, `for`, `foreach`, `wrangle`, or function. A block's variables end with it, and a later `let` of the same name in the same block replaces the earlier one; a `let` in an inner block shadows outer names, including parameters.

`const name = value` declares an immutable value in the same scopes. Reassignment, compound assignment, redeclaring that name in the same block, or passing it through `ref` is an error. An inner block may shadow it. Prefer `const` for lookup tables, fixed calculations, and finished geometry; use `let` for accumulators and geometry or instance builders that will be edited.

Geometry and instance constants freeze the shared value, including aliases created before or after the declaration. Mutation built-ins and wrangle attribute writes through any alias fail. Read-only inspection and wrangles remain valid. `copy(value)`, `geometry(existingGeometry)`, and existing operations that make copies produce independent editable values; ordinary function arguments also remain independent copies. A `let` alias alone does not unfreeze a value:

```text
let builder = box(2, 3, 2)
const original = builder
let edited = copy(original)
translate(edited, vector(4, 0, 0))
output("Original", original)
output("Edited", edited)
```

Records, lists, and their nested fields already have immutable value semantics. A live wrangle element or module handle cannot initialize a constant; capture an element field or an exported module value instead. `ref` arguments are checked for assignability and frozen resources before the function body runs.

`name = value` assigns the nearest existing variable, and `+=`, `-=`, `*=`, and `/=` update it, so loops accumulate without scratch geometry:

```text
let total = 0
for floor_index in 0..floors {
  total += floor_height(floor_index)
}
```

Assigning a name nobody declared is an error (`declare it with let before assigning it`). Parameters, `for` indices, `foreach` items, wrangle bindings and `index`, `deform`'s per-vertex inputs, the constants `pi`, `tau`, and `e`, enums, and module aliases are read-only. Wrangles assign element attributes with the same operators (`point.height += 1`).

### Control Flow
`if condition { … } else if condition { … } else { … }` runs the first branch whose condition is true.

`match value { case a, b { … } case c { … } else { … } }` runs the first case with a value equal to the subject under the equality rules above, trying values left to right; `else` is optional and must come last, and a match needs at least one case. Case values are ordinary expressions, so `case Kind.Tower` and `case "Brick"` both work.

`for index in start..end { … }` visits ascending integer indices, excluding `end`. `for index in 0..arraySize` is the standard indexed loop. Both bounds evaluate once, left to right, and must be finite integers in ±9,007,199,254,740,991. Equal or descending bounds produce an empty loop; at most 150,000 iterations are allowed.

`foreach item in collection { … }` visits list elements, or vector/color components, in order. The collection evaluates once; rebinding its original variable does not change the values being visited. Each item has the same representation as indexed access, including nested records and lists. Empty collections run no iterations. Text, records, geometry, and retained instance collections are not iterable; use `wrangle` for geometry and instance attribute domains.

```text
const placements = [{x: 0, height: 2}, {x: 3, height: 4}]
let towers = geometry()
foreach placement in placements {
  append(towers, place(box(1, placement.height, 1), vector(placement.x, placement.height/2, 0)))
}
output("Towers", towers)
```

Both loop bindings are read-only and scoped to a fresh block for each iteration. Both enforce the 150,000-iteration limit and share the evaluation operation budget. The keywords are distinct: `for` requires a range, and `foreach` requires a collection. The retired `repeat count as index` spelling reports a migration error; replace it with `for index in 0..count`.

`wrangle domain in geometry as element { … }` visits every element of an attribute domain; see [Wrangles](#wrangles).

### Functions
```text
fn panel(center, size, tint) {
  return color(place(unit_quad, center, vector(0, 0, 0), size), tint)
}

fn addWall(ref shell, height) {
  append(shell, translate(box(4, height, 0.2), vector(0, height/2, 0)))
}

addWall(ref building, 3)
```

`fn` declares a function at the top level of a script or module; declarations are hoisted, so a function can be called before (or after) the line that declares it, and functions can call each other and recurse. `return value` ends the call with a value; a bare `return`, or reaching the end, returns nothing, and using such a call as a value is an error (`f() doesn't return a value.`). Calls pass exactly the declared number of arguments. A function may not reuse a built-in's name, and its name must be unique in its module.

Arguments are copies: geometry and instance collections are duplicated on the way in, so a function never edits the caller's value by accident (numbers, text, records, and resources are immutable, so they are shared). A parameter declared `ref` edits the caller's variable instead: the call must pass `ref variable`, and the variable receives the parameter's final value when the call returns. `ref` needs an assignable variable, so a parameter, loop counter, or constant cannot be passed by reference.

A function body sees its own parameters and blocks plus its module's script-level bindings: parameters, constants, enums, module aliases, and the `let` variables that exist when it is called. It can read those script-level variables but cannot assign them (`return a value or pass it with ref`). Functions cannot declare `param`, `project`, `enum`, `fn`, or `use`. Active calls nest at most 32 deep.

### Modules
```text
use "building-kit.form" as kit

let building = kit.header(kit.Kind.Block, 4.4)
append(shell, kit.boxPart(center, size, vector(0, 0, 0), wall, chunk))
```

`use "path.form" as alias` loads another script as a module at the top level, and the script reads its declarations through the alias: `alias.function(…)`, `alias.Enum.member`, and `alias.constant` for its script-level `const` and `let` values. Modules share code between scripts in one project.

- A module may contain only `fn`, `enum`, `const`, `let`, and `use` declarations; anything that would produce geometry (outputs, parameters, project metadata, or other statements) belongs to the script that uses it.
- Paths are relative to the folder of the file that declares the `use`, use forward slashes, name a `.form` file, and may climb with `..` but never above the loader's root. Absolute paths, backslashes, and drive letters are rejected.
- Each module loads once per script, however many files use it, so its enums are one definition everywhere. A cycle of `use` declarations is an error.
- A module's top-level `const` and `let` statements run once, in dependency order, before the using script's first statement. They share the evaluation's random stream, so a `rand()` in a module constant consumes draws before the script's own `seed` runs.
- A module function runs against its own module's script-level bindings, never the caller's.
- A script loads at most 32 files, counting itself, through `use` chains at most 16 files deep. Each module has its own source and token budget; evaluation limits are shared.

Modules are resolved while parsing, so evaluation stays deterministic and never reads files. `CompiledScript::parse` rejects `use`; hosts call `CompiledScript::parse_with_modules(source, path, loader)` with the script's own root-relative path and a `ModuleLoader` (any `Fn(&str) -> Result<String, String>` closure) that returns a normalized path's text. A module failure is reported at the `use` that reached it, with the module's path, line, and column in the message. `CompiledScript::module_paths()` lists every loaded module so hot-reloading hosts can watch them. FORM EDITOR, Joy Editor, `FormInstance::parse_script`, and `FormAssetLibrary` all read modules relative to the project or library root.

## Parameters
`param name = default meta(…)` exposes a tunable value at the top level of the main script (not inside blocks, functions, or modules). The default's type selects the parameter type; parameter values are read-only in the script, and later parameters may use earlier ones in their defaults and bounds.

| Parameter | Declaration | Script reads | Control |
| --- | --- | --- | --- |
| Number | `param width = 8 meta(min=4, max=30, step=0.25)` | A number clamped to the bounds | Slider and field |
| Whole number | `param floors = 2 meta(min=1, max=8, step=1, kind="int")` | A whole number (overrides round, then clamp) | Integer field |
| Seed | `param variation = 7 meta(kind="seed")` | A whole number, 0–2,147,483,647 unless bounded | Field with a reroll button |
| Vector | `param offset = vector(0, 1, 0) meta(min=-5, max=5, step=0.1)` | A vector, each component clamped | XYZ group |
| Color | `param tint = rgba(0.93, 0.42, 0.22, 1)` | A color (bounds default to 0–1, step 0.01; raise `max` for HDR) | Color swatch and RGBA group |
| True/false | `param ridges = true` or `param ridges = 1 meta(kind="bool")` | `true` or `false` | Toggle |
| Text | `param label = "Gate"` | Text, one line of at most 128 characters | Text field |
| Choice by options | `param style = "Brick" meta(options=["Brick", "Glass"])` or a zero-based index default | The chosen option's text | Choice |
| Choice by enum | `param kind = Kind.Tower` | The chosen enum member | Choice |
| Mesh | `param piece = meshBox(1, 1, 0.12)` | A mesh resource | Mesh source |
| Material | `param finish = materialPbr(rgba(0.66, 0.48, 0.3, 1), 0, 0.85)` | A material resource | Material editor |

`meta(…)` fields may appear in any order: `min`, `max`, and `step` (required for numbers and vectors, applied to every component), `label` (at most 48 characters), `group` (at most 32), `info` (one line of hover help, at most 160), `kind` (`int`, `bool`, `seed`, or `enum`), and `options` (2–32 labels of at most 32 characters, implying `enum`). Text fields are double-quoted single lines. A default outside its bounds, `min` above `max`, or a nonpositive `step` is an error. True/false and choice parameters derive their bounds and reject `min`, `max`, and `step`; text, mesh, and material parameters accept neither bounds nor a kind, and enum-typed parameters take their options from the enum. The legacy positional form `param width = 8, 4, 30, 0.25` (default, minimum, maximum, step) still parses.

Overrides are a JSON object keyed by parameter name; unknown names are ignored. Numbers clamp to the bounds without step quantization (whole-number kinds round first), vectors and colors must match their component count, true/false parameters accept `true`, `false`, 0, or 1, choices accept an option or member name or its index, text overrides must be one line of at most 128 characters, and mesh or material overrides use the serialized resource records. A mismatched override is an error at the parameter's declaration.

Every evaluation returns one descriptor per parameter in declaration order (`GeometryData.parameters`): `name`, `type` (`scalar`, `vector`, `color`, `text`, `mesh`, or `material`), bounds and `step` where they apply, `defaultValue`, the resolved `value`, and the presentation fields `label`, `group`, `info`, `kind`, and `options`. True/false and choice parameters are `scalar` descriptors storing 0/1 or the chosen index, so editors and hosts that pass numbers keep working. `parameter_control`, `parameter_label`, `parameter_group`, and `parameter_info` read descriptors, so FORM EDITOR, Joy Editor, and games present controls identically.

## Project Metadata
`project meta(name="…", info="…", title="…")` names the asset once, at the top level of the main script. `name` is required, nonempty, single-line text of at most 120 characters; it names documents and generated files. `info` (at most 160 characters) is the viewport's descriptive line and `title` (at most 120) its display heading. The record is `GeometryData.project`, or `null` without a declaration. `project_name_tokens(source)` locates each quoted name so editors can rename a project without reformatting its metadata.

## Geometry
Geometry values are explicit. Constructors return new geometry, operations take geometry as their first argument, and nothing reaches the result unless the script passes it to `output`. There is no drawing state, current color, or current material.

A geometry holds points (each with a `position`), triangular faces over point indices, attributes on points, faces, and face corners, and a table of the materials its faces use. Variables refer to geometry by identity: `let b = a` aliases it and edits through either name show in both, while `copy(a)` makes an independent value. Function arguments, `merge`, `place`, `instances`, and outputs all take copies.

- **Constructors:** `geometry()` (empty), `geometry(mesh)` or `geometry(other)` (an editable copy), `box(width, height, depth)` (centered; 8 shared corners, flat shaded), `sphere(radius[, segments])` (centered UV sphere with smooth normals and UVs; 3–96 segments, default 20), `grid(columns, rows, width, depth)` (a flat XZ grid of 1–256 cells per axis, with UVs), `revolve(profile, sides)` (spins `(radius, height, 0)` profile points around Y), `tube(path, radius, sides)` (sweeps a circle along path points), `scatter(geometry, count, seed[, weight])`, and `smoothNormals(geometry)` (a copy with averaged point normals). Profiles and paths are geometry with 2–512 ordered, distinct points.
- **In-place edits that return the edited geometry**, so calls nest (`material(color(place(unit, center, turn, size), white), trim)`): `translate(g, offset)`, `rotate(g, radians)` (XYZ Euler angles), `scale(g, factors)` (nonzero; a mirroring scale reverses winding), `color(g, rgba)` (every point takes the color, replacing face and corner colors), `material(g, material)`, and `append(target, source)` (adds a copy of `source`). Rotation and scale also transform authored normals. Edits need geometry; pass a mesh through `geometry(mesh)` first, except `translate`, `rotate`, and `scale`, which return a new geometry when given a mesh.
- **Combining:** `merge(a, b, …)` returns new geometry holding every argument in order; `place(source, position[, rotation, scale])` returns a scaled, rotated, then translated copy (a mesh keeps its exact vertex order, normals, and UVs); `realize(instances)` returns an instance collection as plain geometry. Combining keeps every attribute: a custom attribute one part lacks is filled with its default, shading attributes stored in different domains move to face corners so neither part changes appearance, uncolored parts keep the default FORM tint, and material tables merge. Attributes of the same name must have the same type in every part.
- **Topology:** `addPoint(g, position[, record])` returns the new point's index; the optional record sets point attributes, declaring any the geometry lacks with their default on every other point (`addPoint(sites, vector(x, 0, z), {kind: "tree", object: "assets/objects/tree.joyobject", seed: 4})`). `addFace(g, a, b, c)` adds a triangle and returns its index; `removePoint(g, index)` removes a point and the faces that use it, and `removeFace(g, index)` removes a face. `pointCount(g)` and `faceCount(g)` count elements.
- **Mesh resources:** `meshBox`, `meshSphere`, `transformMesh(mesh, position, rotation, scale)`, and `deform(mesh, x, y, z)` build immutable meshes. `deform` evaluates its three coordinate expressions once per vertex with `x`, `y`, `z`, `u`, `v`, and `index` bound to that vertex and resets normals to flat. Meshes become geometry through `geometry(mesh)`, `place`, the transform operations, `merge`, or `output`.

### Attributes
Attributes live in three domains: `points`, `faces`, and `corners` (three per face, in face order). Instance collections add `instances`, their site points. Each attribute holds one type for every element of its domain:

| Type | Values | Default |
| --- | --- | --- |
| Number | One number | 0 |
| Vector | Three numbers | `vector(0, 0, 0)` |
| Color | Four numbers | zero |
| Text | Text of at most 256 characters, or an enum member stored by name | `""` |
| True/false | `true` or `false` | `false` |
| Material | A face's material (faces `material` only) | none (Default) |

`addAttribute(g, domain, name, value)` declares an attribute outside wrangles, giving every existing element `value`; elements added later receive the type's default. `setAttribute(g, domain, name, index, value)` writes one element, `attribute(g, domain, name, index)` reads one, `hasAttribute` tests for one, and `removeAttribute` removes one (positions cannot be removed). Writes must keep the attribute's type. Names are identifiers of 1–64 characters, at most 64 attributes per domain; records and lists cannot be attributes.

Realization reads these standard names: `position` (points only), `normal` (vector; zero means flat), `uv` (vector whose `x` and `y` are U and V), `color` (RGBA with nonnegative RGB and alpha in 0–1), the faces `material`, and `rotation` (XYZ Euler radians) and `scale` (nonzero XYZ factors). Standard names keep their numeric shape, so `rotation` and `scale` are always XYZ vectors. Instance sites consume `rotation` and `scale` to place their copies; on ordinary points they realize as vertex attributes like custom ones, which is how placements carry them. Every other name is a custom attribute.

`material(g, value)` and `addAttribute(g, "faces", "material", value)` assign a material to every face, and `face.material = value` in a faces wrangle or `setAttribute(g, "faces", "material", index, value)` assigns one. The value is a material resource (`materialPbr(…)` or a material parameter), the name of a palette material as text (`material(g, "Paint")`, which must exist in the evaluation's palette), or `null` for none. Faces without a material draw with the palette's Default. Materials belong to faces: `material(g, m)` leaves isolated points on Default, and isolated points take a material only through an instance collection's material. Script materials join the result's palette once per distinct record, named `ScriptMaterial_<n>`; one geometry's faces reference at most 128 distinct materials.

### Wrangles
```text
let terrain = grid(32, 32, 20, 20)
addAttribute(terrain, "points", "density", 0)
wrangle points in terrain as point {
  const height = noise(point.position.x*0.3, point.position.z*0.3, 7777)
  point.position += vector(0, height*2, 0)
  point.density = smoothstep(-0.4, 0.6, height)
}
```

`wrangle points|faces|corners in geometry as element { … }` runs its block once per element, and `wrangle instances in collection as site { … }` once per site. `element.name` reads and assigns the element's attributes, which must already be declared; `element.index` and `index` give its position. Inside a wrangle, topology edits (`addPoint`, `addFace`, `removePoint`, `removeFace`, `append`), attribute declarations and `setAttribute`, whole-geometry `color` and `material`, outputs, and nested wrangles are errors, so element indices stay stable; query other geometry with `attribute(…)`. Moving points invalidates face and corner normals when the wrangle ends, along with point normals the same wrangle did not write.

### Instances
`instances(source, sites[, material])` places a copy of `source` (geometry or a mesh) at every point of `sites`; both are copied when the collection is created. Each site's `rotation`, `scale`, and `color` attributes orient, size, and tint its copy (a site color multiplies the source's own colors), the optional collection material overrides the source's materials, and custom site attributes tag every vertex of that copy. Declare site attributes on `sites` before creating the collection, or afterward with `addAttribute(collection, "instances", name, value)` and an `instances` wrangle. `output` and `merge` realize collections; `realize(collection)` returns the same geometry for further editing.

## Outputs and Realization
`output("Name", value)` adds geometry, a mesh, or an instance collection to the evaluation's result as a named, visible output; `inspect("Name", value)` records a snapshot for inspection without drawing it. Names are unique, nonempty, and at most 64 characters, and a script declares at most 16 outputs and inspections together. Each takes a snapshot when it is called, so later edits never change it. The evaluation result is the visible outputs, realized in call order; a script without outputs evaluates to empty geometry.

Realization turns geometry into `GeometryData`'s indexed vertex arrays:

- Each vertex reads `normal`, `uv`, and `color` from the most specific domain that stores them (corner, then face, then point). Geometry splits shared points into separate vertices only where faces or corners carry shading or corners carry custom attributes. Normals are normalized; a zero normal draws flat.
- Geometry without a `color` attribute draws in the default FORM tint, `rgba(0.929, 0.424, 0.224, 1)`. Colored and uncolored parts merge without changing either's appearance. Vertex RGB multiplies the material's base color.
- Custom point and corner attributes (and point `rotation` and `scale` outside instance sites) become `GeometryData.vertex_attributes`, one value per vertex (corner values override point values, and instance site values override both); custom face attributes become `GeometryData.triangle_attributes`, one value per triangle. Each is an `AttributeColumn` (`Number { size, values }`, `Text { values }`, or `Bool { values }`); vertices or triangles from parts that lacked an attribute hold its default. `GeometryData::vertex_record(index)` and `triangle_record(index)` return one element's custom attributes as a JSON record.
- Points no face uses are isolated points, listed in `GeometryData.points` with `point_materials`. Isolated points that set at least one attribute to a value other than its default are placements: hosts read them with `joy_engine::rendering::form_placements` (`kind` names their purpose, `object` a `.joyobject` template, and point `rotation` and `scale`, both XYZ vectors, orient and size the spawned instance), and levels spawn the named objects as entities (see [scenes](scenes.md#form-assets-in-levels)).

`GeometryData.outputs` lists every output and inspection in call order as a `NamedOutput { name, visible, value, geometry }`: `visible` is true for `output`, `geometry` is that value realized alone with the evaluation's palette, and `value` is an `OutputValue` whose `json()` returns the documented snapshot record. A geometry snapshot is `{"kind": "geometry", "faces": [[a, b, c], …], "attributes": {"points": {…}, "faces": {…}, "corners": {…}}, "materials"?: […]}`, with each attribute as `{"size", "values", "type"?}` (`"type"` is `"text"`, `"bool"`, or `"material"` when not numeric, and `materials` appears when faces carry the material attribute). An instance snapshot is `{"kind": "instances", "source": …, "sites": …, "material": … | null}`. `OutputValue::is_instances()` tells them apart without converting. Snapshots convert to JSON only when read or serialized, so read a large one once and keep it.

## Built-in Functions
The table is generated from the engine's language catalog; run `node scripts/form-reference.mjs --write` after changing `BUILTINS`, and `npm run test:tooling` fails while it is stale. Arguments in brackets are optional.
<!-- form-builtins:start -->
| Function | Summary |
| --- | --- |
| **Numbers** | |
| `sin(radians)` | Sine of an angle in radians. |
| `cos(radians)` | Cosine of an angle in radians. |
| `tan(radians)` | Tangent of an angle in radians. |
| `atan2(y, x)` | Angle in radians of the direction (x, y). |
| `abs(value)` | Absolute value. |
| `sqrt(value)` | Square root. |
| `floor(value)` | Largest whole number at or below the value. |
| `ceil(value)` | Smallest whole number at or above the value. |
| `round(value)` | Nearest whole number; halves round up. |
| `exp(value)` | e raised to the value. |
| `log(value)` | Natural logarithm. |
| `pow(base, exponent)` | Base raised to the exponent (the same as base ^ exponent). |
| `min(a, b)` | The smaller of two numbers. |
| `max(a, b)` | The larger of two numbers. |
| `clamp(value, low, high)` | The value limited to low–high. |
| `mix(a, b, t)` | Linear blend from a to b by t; works on numbers, vectors, and colors. |
| `smoothstep(low, high, value)` | Smooth 0–1 ramp as the value moves from low to high. |
| `noise(x, y, z)` | Deterministic smooth value noise in -1–1. |
| `rand()` | Next number in 0–1 from the script's random stream. |
| `seed(value)` | Restart the script's random stream from a whole-number seed. |
| `number(flag)` | 1 for true and 0 for false. |
| `itemCount(list)` | How many items a list or record holds. |
| **Vectors and Colors** | |
| `vector(x, y, z)` | An XYZ vector. |
| `rgba(red, green, blue, alpha)` | A linear RGBA color; RGB may exceed 1 for glow. |
| `length(vector)` | Length of a vector. |
| `normalize(vector)` | The vector scaled to length 1. |
| `dot(a, b)` | Dot product of two vectors. |
| `cross(a, b)` | Cross product of two vectors. |
| **Mesh and Material Resources** | |
| `meshBox(width, height, depth)` | A reusable box mesh resource. |
| `meshSphere(radius[, segments])` | A reusable sphere mesh resource. |
| `transformMesh(mesh, position, rotation, scale)` | A transformed copy of a mesh resource. |
| `deform(mesh, x, y, z)` | A mesh whose vertices move to per-vertex x, y, and z expressions. |
| `materialPbr(color, metallic, roughness)` | A physically based material resource. |
| **Geometry** | |
| `geometry([mesh])` | Empty editable geometry, or an editable copy of a mesh or geometry. |
| `box(width, height, depth)` | A centered box: 8 corners, flat shaded. |
| `sphere(radius[, segments])` | A centered UV sphere with smooth normals. |
| `grid(columns, rows, width, depth)` | A flat XZ grid with UVs. |
| `revolve(profile, sides)` | Spin a (radius, height, 0) profile around the Y axis. |
| `tube(path, radius, sides)` | Sweep a circle along a path of points. |
| `place(source, position[, rotation, scale])` | A scaled, rotated, and moved copy of a geometry or mesh. |
| `merge(a, b, …)` | New geometry combining every argument, attributes and materials included. |
| `append(target, source)` | Add a copy of source to target in place; returns target. |
| `realize(instances)` | Geometry holding a copy of an instance collection's source per site. |
| `copy(value)` | An independent copy of geometry or an instance collection. |
| `translate(geometry, offset)` | Move geometry in place; returns it. |
| `rotate(geometry, radians)` | Rotate geometry in place by XYZ Euler angles; returns it. |
| `scale(geometry, factors)` | Scale geometry in place per axis; returns it. |
| `color(geometry, rgba)` | Color every point of geometry in place; returns it. |
| `material(geometry, material)` | Assign a material (or palette material name) to every face; returns the geometry. |
| `smoothNormals(geometry)` | A copy with averaged point normals. |
| `addPoint(geometry, position[, attributes])` | Add a point, optionally setting attributes from a record; returns its index. |
| `addFace(geometry, a, b, c)` | Add a triangle between three point indices; returns its index. |
| `removePoint(geometry, index)` | Remove a point and the faces that use it. |
| `removeFace(geometry, index)` | Remove a face. |
| `pointCount(geometry)` | How many points geometry has. |
| `faceCount(geometry)` | How many faces geometry has. |
| **Attributes** | |
| `addAttribute(geometry, domain, name, default)` | Declare an attribute on points, faces, corners, or instances. |
| `setAttribute(geometry, domain, name, index, value)` | Set one element's attribute value. |
| `attribute(geometry, domain, name, index)` | Read one element's attribute value. |
| `hasAttribute(geometry, domain, name)` | Whether geometry declares an attribute. |
| `removeAttribute(geometry, domain, name)` | Remove a declared attribute. |
| **Distribution and Instances** | |
| `scatter(geometry, count, seed[, weight])` | Points sampled over a surface, optionally weighted by an attribute. |
| `instances(source, sites[, material])` | A collection placing source at every site point. |
| **Results** | |
| `output(name, geometry)` | Add geometry to the result as a named, visible output. |
| `inspect(name, value)` | Record a named snapshot for inspection without drawing it. |
<!-- form-builtins:end -->

`addPoint` also takes an optional third argument, a record of point attribute values (see [Geometry](#geometry)). `materialPbr` takes an RGBA base color, metallic, and roughness, each in 0–1. `scatter` samples triangle area, weighted by a nonnegative number point attribute when `weight` names one; it interpolates point attributes, generates face normals, and uses its own seeded stream, so it never consumes `rand()` draws. `rand()` and `seed(value)` drive one deterministic stream per evaluation, starting from seed 1; seeds are whole numbers. `noise(x, y, z)` is smooth, deterministic value noise in −1–1, and `smoothstep` requires `low < high`.

## Rust API
```rust
use joy_engine::form::{CompiledScript, FormInstance, mesh_box};
use serde_json::json;

let script = CompiledScript::parse(source)?;
let geometry = script.evaluate(&json!({"width": 8}), None)?;
for material in geometry.form_materials() {
    // Typed name, base color, metallic, roughness, emissive, alpha mode, cutoff, and textures.
}

// A script that uses modules, read relative to a project root.
let read = |path: &str| std::fs::read_to_string(root.join(path)).map_err(|error| error.to_string());
let city = CompiledScript::parse_with_modules(&block_source, "assets/form/city/block.form", &read)?;

// An instance and one of its variants.
let instance = FormInstance::read(&instance_text)?;
let tower = instance.parse_script("assets/form/city/tower.forminst", &read)?;
let tall = tower.evaluate(&serde_json::Value::Object(instance.overrides_for(Some("Tall"))?), None)?;
let unit_box = mesh_box(1.0, 1.0, 1.0)?;
```

- `CompiledScript::parse(&str) -> Result<CompiledScript, Diagnostic>` validates syntax and source budgets without executing anything; `parse_with_modules(source, path, &dyn ModuleLoader)` also loads modules. `CompiledScript::evaluate(&self, overrides, materials) -> Result<GeometryData, Diagnostic>` runs it; `compile(source, overrides, materials)` does both for a script without modules.
- `overrides` is a JSON object keyed by parameter name (see [Parameters](#parameters)). `materials` is an optional palette of serialized material records; `None` yields the single Default material.
- `Diagnostic { message, line, column }` implements `Error` and `Display`. Lines are one-based and columns count characters from one, matching FORM EDITOR's script editor. Failures without a source position report line 1, column 1; failures inside a module are reported at the `use` or call that reached them, naming the module's path, line, and column.
- `GeometryData` holds indexed, Y-up geometry in script units: flat `positions`, `normals` (zero means flat-shaded), `uvs`, `colors` (RGB), `alphas`, `triangles`, `points`, `triangle_materials` and `point_materials` (palette indices), serialized `materials`, `parameters`, `project`, `vertex_attributes`, `triangle_attributes`, and `outputs` (`Option<Vec<NamedOutput>>`). JSON uses camelCase field names (`triangleMaterials`, `vertexAttributes`, …); empty attribute maps and absent outputs are omitted, and an absent project is `null`. `to_mesh()` converts the triangles to an engine `Mesh` with one material slot per palette entry.
- `GeometryData::form_materials()` returns typed `FormMaterial` records (with `AlphaMode` serialized as `OPAQUE`, `MASK`, or `BLEND`, and optional embedded texture URIs) in palette order, so triangle and point material indices address it directly. Missing or malformed fields fall back to `default_material` values.
- Tools validate document values with the same rules evaluation uses: `read_materials` (palette with Default first, 128 materials, embedded PNG, JPEG, or WebP maps within `DOCUMENT_TEXTURE_BYTES` each and `DOCUMENT_TOTAL_TEXTURE_BYTES` together), `default_material`, `read_parameter_value` (scalars, XYZ vectors, RGBA colors, true/false, one line of text, and mesh or material resources, with integral numbers kept as JSON integers), `read_mesh`, `read_material_value`, and the resource constructors `mesh_box_value` and `mesh_sphere_value`; failures are `FormValueError`s.
- `form_language_catalog()`, `FORM_KEYWORDS`, and `FORM_ATTRIBUTE_DOMAINS` describe the language for highlighting, completion, and hover text, and `form_document_symbols(source, cursor)` lists the names a script declares that are in scope at a character offset (variables, parameters, functions, enums, modules, and attributes). The [authoring kit](rust-authoring.md#script-editor) uses both.
- `mesh_box(width, height, depth)` and `mesh_sphere(radius, segments)` return indexed primitive geometry (`DEFAULT_SPHERE_SEGMENTS` is 20), with FORM's default vertex tint and Default material. They return a `Diagnostic` for nonpositive sizes or out-of-range segment counts instead of panicking.
- Rendering lives in `joy-scene`: `compile_form_mesh`, `form_mesh_asset`, and `FormAssetLibrary` turn results into mesh assets, and `FormAssetLibrary::resolve_asset(path, text, read, variant, parameters, model)` compiles a `.form` or `.forminst` with its referenced script and modules (read through `read`) and returns the mesh with its placements. See [scenes](scenes.md).

## Ownership
Every evaluation creates its own geometry, random stream, material-identity registry, procedural store, module state, and allocation and work budgets. A `CompiledScript` retains only immutable syntax (including its modules), so evaluations are independent and deterministic for identical inputs. Returned data is owned by the caller and never borrows the overrides, the palette, or earlier results. Geometry bindings alias within an evaluation; `copy`, function arguments, resource conversion, and named outputs take owned snapshots.

## Evaluation Limits
| Limit | Value |
| --- | --- |
| Source per file | 200,000 characters and 40,000 tokens |
| Nesting | 48 blocks and 64 expressions |
| Operations per evaluation | 2,000,000 (statements, expressions, and per-element procedural work) |
| Result geometry | 150,000 vertices and 180,000 triangles |
| Procedural allocation (cumulative, including copies and snapshots) | 150,000 points, 180,000 faces, and 2,000,000 attribute components |
| Mesh resources (cumulative) | 150,000 vertices |
| Attributes | 64 per domain; text values of at most 256 characters |
| Outputs and inspections | 16 per evaluation, 150,000 realized preview vertices together |
| `for` / `foreach` iterations | 150,000 per loop |
| Function calls | 32 nested calls |
| Modules | 32 files per script (counting the script), 16 levels of `use` |
| Records and lists | 250,000 values, 2,000,000 text characters (keys included), and depth 32 per evaluation |
| Coordinates | Finite and within ±1,000,000 |
| Materials | 128 per palette and per geometry's material table; textures 4 MB each and 32 MB per palette |

The limits are fixed by the engine; hosts cannot raise or lower them yet. Flat binary and component chains are evaluated and dropped iteratively, so documents near the token limit do not overflow a native or browser stack. Authored content never panics; limit violations, malformed values, and unsupported functions return a `Diagnostic`.

## Numerical and Text Boundaries
Quoted text must contain valid Unicode scalar values; unpaired surrogate escapes are rejected. Diagnostics count character columns, and a leading byte-order mark takes no column. Record fields keep their authored order; `__proto__`, `prototype`, and `constructor` stay reserved as record keys and attribute names because exported data reaches JavaScript glTF consumers. Procedural transforms reject nonfinite computed normals at their source location while preserving legitimate zero-normal sentinels. Floating-point JSON may write an integral value as `5.0`; its numerical value remains unchanged. Recorded-reference comparisons use explicit tolerances for transcendental rounding.

## Migrating From FORM v1
FORM v1 drew into an implicit canvas with a current color, material, UV, normal, and user-data record. FORM v2 removes that drawing state: every result is a geometry value passed to `output`. The retired built-ins fail with a migration message that names their replacement:

| Retired | Replacement |
| --- | --- |
| `color(r, g, b[, a])` setting the drawing color | `color(geometry, rgba(r, g, b, a))`; old three-argument colors had alpha 1 |
| `material(name)`, `material(value)`, or `material()` selecting the drawing material | `material(geometry, material)`, with a material resource, a palette name as text, or `null` |
| `box(x, y, z, width, height, depth)` | `translate(box(width, height, depth), vector(x, y, z))` |
| `sphere(x, y, z, radius[, segments])` | `translate(sphere(radius, segments), vector(x, y, z))` |
| `point(x, y, z)` | `addPoint(geometry, vector(x, y, z))` |
| `triangle(ax, ay, az, bx, by, bz, cx, cy, cz)` | `addPoint()` for each corner and `addFace()` |
| `surface(columns, rows, width, depth, height)` | `grid(columns, rows, width, depth)` shaped in a points wrangle |
| `uv(u, v)` and `normal(x, y, z)` | Assign `point.uv` or `point.normal` in a wrangle |
| `userData(record)`, `with userData(record) { … }`, `setUserData(g, index, record)`, and `userData(g, index)` | Point attributes: `addPoint(g, position, record)`, or `addAttribute()` with `setAttribute()`; read them with `attribute()` |
| `instance(mesh, position, rotation, scale[, material])` | `place(mesh, position, rotation, scale)` with `material(geometry, material)`, then `output` |
| `@name` attribute syntax | `wrangle points in geometry as point { point.name … }` |

Other contract changes:
- Geometry drawn before any `color(…)` used the default FORM tint, and uncolored v2 geometry realizes with that same tint. A v1 `output(name, g)` multiplied `g`'s colors by the drawing color and used the drawing material at that moment; apply them explicitly when they were not white and Default.
- Conditions require true/false. `if count { … }` becomes `if count > 0 { … }`, arithmetic flags such as `(a > 0)*(b == 1)` become `a > 0 AND b == 1`, `(1 - flag)` becomes `NOT flag`, and `number(flag)` replaces a comparison used as 0 or 1.
- Parameters with `kind="bool"` now read as `true`/`false`, and option parameters read as their option's text rather than an index; their descriptors still store numbers.
- `let` remains block-scoped, and assignment (`=`, `+=`, `-=`, `*=`, `/=`) replaces scratch-geometry counters. `match`, `enum`, records with enum keys, lists, `fn`, and `use` replace numeric kind codes, if-chains, and copied helpers.
- Per-point records become point attributes; a record attached to every vertex of a triangle (a chunk id, for example) becomes a face attribute (`addAttribute(part, "faces", "chunk", n)`), which realizes as `triangle_attributes`. Records and lists cannot be attributes, so flatten nested data into fields.
- `GeometryData.user_data` (JSON `userData`) is gone; read `vertex_attributes`, `triangle_attributes`, `vertex_record`, `triangle_record`, and placements. Named output snapshots keep their attributes at `["attributes"]["points" | "faces" | "corners"][name]`.
- `.formlab` documents become `.forminst` instances (version 2); reference the script with `"form"` to share one recipe.

## Validation
The tests (`joy-form` unit tests and the `form_*` modules in `crates/joy-form/tests/integration/`, run with `cargo test -p joy-form`) cover parse and evaluation isolation, ownership, parameter clamping and types, booleans and logic, scoping and assignment, functions and modules, enums, records, precedence and numerical rounding, resources and transforms, attributes and their realization, diagnostics and operation limits, procedural topology and attribute rules, wrangles, retained instances, shapes and scatter, instance documents, material and texture validation, typed materials, primitive parity with recorded reference values, and the evaluator's request framing. `npm run test:tooling` checks this guide against the language catalog.

```sh
cargo +nightly fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p joy-form
cargo build --target wasm32-unknown-unknown --lib
node scripts/form-reference.mjs
```

The `joy-form-evaluate` JSON-lines CLI (built with `cargo build -p joy-form --bin joy-form-evaluate`, without the renderer or window stack) evaluates one request per line: `{"source": "…", "overrides": {…}, "materials": [ … ]}`, plus `path` and either `modules` (module paths to source text) or `root` (a folder to read modules from) for scripts that use modules. It returns success geometry with `evaluation_ms`, or a message with line and column; requests are limited to 32 MiB.

FORM EDITOR keeps a 69-case FORM v2 reference corpus (`tools/form-editor/tests/fixtures/form-reference.json.gz`) covering shipped examples, recipes, structures, semantics, and rejections. It was recorded with the v2 evaluator after its geometry was verified against the FORM v1 reference. A retained Node harness builds and drives the CLI and compares geometry, numeric tolerances, and diagnostic locations against those fixtures, writing ignored evidence to `tools/form-editor/artifacts/form-parity`; every case must pass. Run it after changing the evaluator, and re-record a case only for an intentional, explained behavior change.

```bash
npm run test:parity --workspace @joy-games/form-editor
```
