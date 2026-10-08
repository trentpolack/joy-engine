// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

export const EXAMPLES = [
  {
    name:'Wave study', kind:'SURFACE', description:'A rolling landscape, made from a single expression.',
    source:`# WAVE STUDY
project meta(name="Wave study", info="SURFACE STUDY / 001", title="MAKE SOME WAVES.")

# A little math. A little landscape.

param height = 2.4 meta(min=0, max=5, step=0.1)
param frequency = 0.55 meta(min=0.1, max=1.5, step=0.05)
param resolution = 64 meta(min=8, max=120, step=1)
param tint = rgba(0.93, 0.42, 0.22, 1)

with userData({kind: "terrain", study: "wave"}) {
  color(tint)
  surface(resolution, resolution, 24, 24,
    sin(x*frequency)*cos(z*frequency)*height
  )
}

# A scattering of warm points above the surface.
seed(19)
with userData({kind: "accent", study: "wave"}) {
  color(1, 0.82, 0.4)
  repeat 500 as i {
    let px = (rand() - 0.5)*24
    let pz = (rand() - 0.5)*24
    let py = sin(px*frequency)*cos(pz*frequency)*height
    point(px, py + 0.18, pz)
  }
}`,
  },
  {
    name:'Orbital bloom',kind:'POINT CLOUD',description:'A spherical spiral with a few ideas of its own.',
    source:`# ORBITAL BLOOM
project meta(name="Orbital bloom", info="POINT CLOUD / 002", title="POINTS OF POSSIBILITY.")

param count = 16000 meta(min=1000, max=20000, step=1000)
param radius = 8 meta(min=2, max=14, step=0.1)
param ripple = 0.8 meta(min=0, max=3, step=0.1)

let bloom = geometry()
addAttribute(bloom, "points", "color", rgba(1,1,1,1))
repeat count as i {
  let t = i/count
  let y = 1 - 2*t
  let angle = i*2.399963
  let ring = sqrt(1 - y*y)
  let r = radius + sin(angle*0.04)*ripple
  let pointIndex = addPoint(bloom, vector(cos(angle)*ring*r, y*radius, sin(angle)*ring*r))
  setAttribute(bloom, "points", "color", pointIndex, rgba(1 - t*0.5, 0.35 + t*0.4, 0.2 + t*0.6, 1))
}
setUserData(bloom, 0, {kind: "orbit", pointCount: count})
output("Bloom", bloom)`,
  },
  {
    name:'Pocket city',kind:'GEOMETRY',description:'Seeded blocks, built one small decision at a time.',
    source:`# POCKET CITY
project meta(name="Pocket city", info="GEOMETRY / 003", title="SMALL CITY. BIG IDEAS.")

param blocks = 11 meta(min=3, max=18, step=1)
param height = 8 meta(min=1, max=16, step=0.5)
param spacing = 1.8 meta(min=1.2, max=3, step=0.1)
param random_seed = 21 meta(min=1, max=99, step=1)

seed(random_seed)
repeat blocks as x {
  repeat blocks as z {
    let h = ((rand()^2)*height) + 0.5
    let px = (x - ((blocks - 1)/2))*spacing
    let pz = (z - ((blocks - 1)/2))*spacing
    let building = meshBox(1, h, 1)
    color(0.5 + h/height*0.45, 0.35 + rand()*0.2, 0.25)
    with userData({kind: "building", grid: {x: x, z: z}, height: h}) {
      instance(building, vector(px, h/2, pz))
    }
  }
}`,
  },
  {
    name:'Satellite garden',kind:'GEOMETRY',description:'Repeated spheres around a wandering curve.',
    source:`# SATELLITE GARDEN
project meta(name="Satellite garden", info="GEOMETRY / 004", title="A LITTLE SPACE TO PLAY.")

param count = 36 meta(min=6, max=80, step=1)
param loops = 3 meta(min=1, max=8, step=1)
param offset = vector(0, 0, 0) meta(min=-5, max=5, step=0.1)
param radius = 6 meta(min=2, max=10, step=0.1)

repeat count as i {
  let t = i/count
  let a = t*tau*loops
  let satellite = meshSphere((t*0.55) + 0.25, 16)
  color(0.95, 0.35 + t*0.45, 0.2 + t*0.4)
  with userData({kind: "satellite", index: i, orbit: {turns: loops, phase: t}}) {
    instance(satellite, vector(cos(a)*radius + offset.x, (t - 0.5)*14 + offset.y, sin(a)*radius + offset.z))
  }
}`,
  },
];

EXAMPLES.push({
  name: 'Parametric bridge',
  kind: 'RUNTIME RECIPE',
  description: 'One script, replaceable meshes and materials, caller-controlled shape.',
  source: `# PARAMETRIC BRIDGE
project meta(name="Parametric bridge", info="RUNTIME RECIPE / 001", title="GIVE AN IDEA SOME ROOM.")

# The editor and a game supply these same inputs.
param start = vector(-8, 2, 0) meta(min=-20, max=20, step=0.1)
param end = vector(8, 2, 0) meta(min=-20, max=20, step=0.1)
param count = 24 meta(min=4, max=64, step=1)
param width = 3 meta(min=1, max=6, step=0.1)
param sag = 2 meta(min=0, max=6, step=0.1)
param variation = 0.08 meta(min=0, max=0.4, step=0.01)
param random_seed = 42 meta(min=1, max=9999, step=1)
param tint = rgba(0.93, 0.42, 0.22, 1)
param plank = meshBox(1, 0.18, 1)
param finish = materialPbr(tint, 0.15, 0.55)

seed(random_seed)
color(1, 1, 1)
let dx = end.x - start.x
let dz = end.z - start.z
let span = sqrt(dx*dx + dz*dz)
let heading = -atan2(dz, dx)

repeat count as i {
  let t = i / (count - 1)
  let px = mix(start.x, end.x, t)
  let py = mix(start.y, end.y, t) - sin(t*pi)*sag
  let pz = mix(start.z, end.z, t)
  let slope = atan2(end.y - start.y - cos(t*pi)*pi*sag, max(span, 0.01))
  with userData({kind: "bridge-plank", index: i, spanPosition: t}) {
    instance(plank, vector(px, py, pz),
      vector((rand() - 0.5)*variation, heading, slope),
      vector(max(span, 0.1)/count*0.9, 1, width), finish)
  }
}

# Reusable geometry can also be deformed before instancing.
let support = deform(meshBox(0.35, 3, 0.35), x, y + 1.5, z)
material(finish)
with userData({kind: "bridge-support", endpoint: "start"}) { instance(support, start) }
with userData({kind: "bridge-support", endpoint: "end"}) { instance(support, end) }
`
});

EXAMPLES.push({
  name: 'Terrain workshop',
  kind: 'GEOMETRY + ATTRIBUTES',
  description: 'Generate a landscape, paint density with math, and grow a reusable assembly.',
  source: `# TERRAIN WORKSHOP
project meta(name="Terrain workshop", info="GEOMETRY / ATTRIBUTES / ASSEMBLY", title="GROW YOUR OWN WORLD.")

param height = 2.4 meta(min=0, max=5, step=0.1)
param frequency = 0.35 meta(min=0.1, max=0.8, step=0.01)
param count = 80 meta(min=0, max=180, step=1)
param random_seed = 7777 meta(min=1, max=9999, step=1)
param lowColor = rgba(0.12, 0.28, 0.31, 1)
param highColor = rgba(0.47, 0.65, 0.52, 1)
param column = meshBox(0.35, 1, 0.35)
param finish = materialPbr(rgba(0.93, 0.42, 0.22, 1), 0.2, 0.5)

let terrain = grid(32, 32, 22, 22)
addAttribute(terrain, "points", "density", 0)
color(terrain, lowColor)
wrangle points in terrain as element {
  let wave = sin(element.position.x*frequency)*cos(element.position.z*frequency)
  element.position = vector(element.position.x, wave*height, element.position.z)
  element.density = clamp((wave + 0.3)*1.4, 0, 1)
  element.color = mix(lowColor, highColor, element.density)
}
output("Terrain", terrain)

# Sites inherit surface attributes. Inspect density in the lower panel.
let sites = scatter(terrain, count, random_seed, "density")
addAttribute(sites, "points", "scale", vector(1,1,1))
addAttribute(sites, "points", "rotation", vector(0,0,0))
seed(random_seed)
wrangle points in sites as element {
  let tall = 0.4 + rand()*2.2
  element.position = vector(element.position.x, element.position.y + tall/2, element.position.z)
  element.scale = vector(1, tall, 1)
  element.rotation = vector(0, rand()*tau, 0)
  element.color = rgba(1, 1, 1, 1)
}
repeat pointCount(sites) as i {
  setUserData(sites, i, {kind: "column-site", index: i, source: "terrain-scatter"})
}
inspect("Sites", sites)
let columns = instances(column, sites, finish)
output("Columns", columns)
`
});

EXAMPLES.push({
  name: 'Vessel workshop', kind: 'PROFILE', description: 'Turn a handful of profile points into a family of sculpted props.',
  source: `# VESSEL WORKSHOP
project meta(name="Vessel workshop", info="PROFILE / PROP STUDY", title="SHAPE A FAMILY.")

param height = 5 meta(min=2, max=8, step=0.1)
param belly = 2 meta(min=0.8, max=3, step=0.1)
param neck = 0.7 meta(min=0.3, max=1.5, step=0.05)
param flutes = 8 meta(min=0, max=16, step=1)
param detail = 48 meta(min=12, max=64, step=4)
param tint = rgba(0.92, 0.43, 0.24, 1)

# Profile points run bottom to top: X is radius, Y is height.
# A radius-zero first point closes the base; the mouth stays open.
let profile = geometry()
addPoint(profile, vector(0, 0, 0))
addPoint(profile, vector(belly*0.6, 0, 0))
addPoint(profile, vector(belly, height*0.25, 0))
addPoint(profile, vector(belly*0.85, height*0.55, 0))
addPoint(profile, vector(neck, height*0.82, 0))
addPoint(profile, vector(neck*1.15, height, 0))
let vessel = revolve(profile, detail)
repeat pointCount(vessel) as i {
  setUserData(vessel, i, {kind: "vessel-surface", profile: "sculpted"})
}
color(vessel, tint)
wrangle points in vessel as element {
  let angle = atan2(element.position.z, element.position.x)
  let ripple = 1 + sin(angle*flutes)*0.05
  element.position = vector(element.position.x*ripple, element.position.y, element.position.z*ripple)
  element.color = mix(tint, rgba(0.98,0.78,0.47,1), element.position.y/height*0.55)
}
let finished = smoothNormals(vessel)
output("Vessel", finished)
inspect("Profile", profile)
`
});

EXAMPLES.push({
  name: 'Path workshop', kind: 'SWEEP', description: 'Sketch a path in code, then turn it into cables, coils, or stylized pipes.',
  source: `# PATH WORKSHOP
project meta(name="Path workshop", info="PATH / SWEEP STUDY", title="FOLLOW THE CURVE.")

param radius = 3 meta(min=1, max=5, step=0.1)
param turns = 2 meta(min=0.5, max=4, step=0.25)
param rise = 6 meta(min=1, max=10, step=0.1)
param thickness = 0.3 meta(min=0.1, max=0.8, step=0.05)
param segments = 80 meta(min=24, max=128, step=4)
param tint = rgba(0.23, 0.66, 0.66, 1)

let path = geometry()
repeat segments + 1 as i {
  let t = i/segments
  let angle = t*turns*tau
  addPoint(path, vector(cos(angle)*radius, t*rise, sin(angle)*radius))
}
let pipe = tube(path, thickness, 12)
repeat pointCount(pipe) as i {
  setUserData(pipe, i, {kind: "swept-pipe", path: "helix"})
}
color(pipe, tint)
wrangle points in pipe as element {
  element.color = mix(tint, rgba(0.94,0.68,0.34,1), clamp(element.position.y/rise,0,1))
}
output("Pipe", pipe)
inspect("Path", path)
`
});

EXAMPLES.push({
  name: 'Harvest grove',
  kind: 'GAMEPLAY / RESOURCES',
  description: 'Seeded trees with resource IDs, harvest reserves, and regrowth rules for a game to own.',
  source: `# HARVEST GROVE
project meta(name="Harvest grove", info="GAMEPLAY / RESOURCES", title="GROW SOMETHING USEFUL.")

param rows = 4 meta(min=2, max=6, step=1)
param columns = 5 meta(min=2, max=6, step=1)
param spacing = 3.5 meta(min=3, max=5, step=0.1)
param jitter = 0.5 meta(min=0, max=0.8, step=0.05)
param random_seed = 42 meta(min=1, max=9999, step=1)
param reserve = 30 meta(min=5, max=100, step=5)
param regrowSeconds = 45 meta(min=5, max=120, step=5)
param harvestedFraction = 0 meta(min=0, max=1, step=0.05)

# Each point marks a tree's position. Its ID lets a game keep track of that tree.
let nodes = geometry()
addAttribute(nodes, "points", "remaining", 0)
addAttribute(nodes, "points", "scale", vector(1,1,1))
seed(random_seed)
repeat rows as row {
  repeat columns as column {
    let px = (column - (columns - 1)/2)*spacing + (rand() - 0.5)*jitter
    let pz = (row - (rows - 1)/2)*spacing + (rand() - 0.5)*jitter
    let size = 0.75 + rand()*0.5
    let id = addPoint(nodes, vector(px, 0, pz))
    let resourceData = {
      kind: "resource-node",
      id: id,
      resource: "timber",
      capacity: reserve,
      regrowSeconds: regrowSeconds
    }
    setUserData(nodes, id, resourceData)
    setAttribute(nodes, "points", "remaining", id, reserve)
    setAttribute(nodes, "points", "scale", id, vector(size,size,size))
  }
}
wrangle points in nodes as node {
  # Preview harvesting every tree. A game can harvest individual trees instead.
  node.remaining = node.remaining*(1 - harvestedFraction)
}
inspect("Resource nodes", nodes)

# Choose "Resource nodes" to inspect each tree's data.
# A game can reduce remaining as wood is collected, then refill it to capacity
# after regrowSeconds. Keep that progress in the game; rerunning this script starts fresh.

let trunk = box(0.35, 1.5, 0.35)
translate(trunk, vector(0,0.75,0))
color(trunk, rgba(0.38,0.21,0.12,1))
output("Trunks", instances(trunk, nodes))
let crown = geometry(deform(meshBox(1.9,2.4,1.9), x*(0.7 - y*0.4), y + 2.2, z*(0.7 - y*0.4)))
color(crown, rgba(0.25,0.65,0.43,1))
let foliage = copy(nodes)
wrangle points in foliage as tree {
  # Harvested trees keep a little foliage so they're still easy to find.
  tree.scale = tree.scale*(0.25 + 0.75*tree.remaining/reserve)
}
output("Canopies", instances(crown, foliage))
let ground = box(columns*spacing,0.2,rows*spacing)
translate(ground,vector(0,-0.15,0))
color(ground,rgba(0.16,0.24,0.23,1))
output("Ground",ground)
`
});

EXAMPLES.push({
  name: 'Patrol circuit',
  kind: 'GAMEPLAY / NAVIGATION',
  description: 'A closed route with ordered waypoints, arrival radii, movement speeds, and pauses.',
  source: `# PATROL CIRCUIT
project meta(name="Patrol circuit", info="GAMEPLAY / NAVIGATION", title="SET THE PATROL.")

param count = 12 meta(min=4, max=24, step=1)
param radius = 7 meta(min=3, max=12, step=0.1)
param bend = 0.2 meta(min=0, max=0.35, step=0.05)
param rise = 1.5 meta(min=0, max=3, step=0.1)
param speed = 2 meta(min=0.5, max=6, step=0.1)
param pauseSeconds = 1.5 meta(min=0, max=5, step=0.1)
param arrivalRadius = 0.4 meta(min=0.1, max=0.8, step=0.05)

let waypoints = geometry()
addAttribute(waypoints,"points","speed",speed)
addAttribute(waypoints,"points","pauseSeconds",0)
color(waypoints,rgba(1,1,1,1))
repeat count as i {
  let angle = i/count*tau
  let distance = radius*(1 + sin(angle*3)*bend)
  let location = vector(cos(angle)*distance, 1 + (sin(angle*2) + 1)*rise/2, sin(angle)*distance)
  let id = addPoint(waypoints,location)
  let waypointData = {
    kind: "waypoint",
    id: id,
    route: "perimeter",
    nextId: (i + 1)%count,
    arrivalRadius: arrivalRadius
  }
  setUserData(waypoints,id,waypointData)
}
wrangle points in waypoints as waypoint {
  waypoint.speed = speed*(0.75 + 0.25*cos(waypoint.index/count*tau))
  waypoint.pauseSeconds = 0
  if waypoint.index%3 == 0 { waypoint.pauseSeconds = pauseSeconds }
  waypoint.color = mix(rgba(0.18,0.65,0.7,1),rgba(1,0.65,0.2,1),waypoint.pauseSeconds/max(pauseSeconds,0.01))
}
inspect("Waypoints",waypoints)

# Choose "Waypoints" to inspect the route. A game can move an agent from ID 0
# to each nextId, looping back to the start. Speed is in units per second.
# arrivalRadius says how close counts as arriving; pauseSeconds says how long to wait.
# The game handles movement and obstacles. The line below just shows the route.

let path = geometry()
repeat count as i { addPoint(path,attribute(waypoints,"points","position",i)) }
addPoint(path,attribute(waypoints,"points","position",0))
let guide = tube(path,0.08,6)
color(guide,rgba(0.22,0.45,0.52,1))
output("Route guide",guide)
output("Waypoint markers",instances(meshSphere(0.3,6),waypoints))
let center = box(radius*0.7,0.3,radius*0.7)
color(center,rgba(0.2,0.28,0.36,1))
output("Center platform",center)
`
});

EXAMPLES.push({
  name: 'Tactical terraces',
  kind: 'GAMEPLAY / GRID',
  description: 'Stepped terrain with tile IDs, grid coordinates, traversal costs, and floodable cells.',
  source: `# TACTICAL TERRACES
project meta(name="Tactical terraces", info="GAMEPLAY / GRID", title="MAKE EVERY TILE MATTER.")

param rows = 8 meta(min=3, max=12, step=1)
param columns = 8 meta(min=3, max=12, step=1)
param cellSize = 1.5 meta(min=1, max=2.5, step=0.1)
param elevation = 3 meta(min=0.5, max=5, step=0.1)
param waterLevel = 1.5 meta(min=0, max=3, step=0.1)
param random_seed = 19 meta(min=1, max=9999, step=1)

let tiles = geometry()
addAttribute(tiles,"points","height",0)
addAttribute(tiles,"points","walkable",1)
addAttribute(tiles,"points","moveCost",1)
addAttribute(tiles,"points","scale",vector(1,1,1))
color(tiles,rgba(1,1,1,1))
repeat rows as row {
  repeat columns as column {
    let field = noise(column*0.3,row*0.3,random_seed)
    let height = 0.3 + floor((field + 1)*elevation*2)/4
    let id = addPoint(tiles,vector((column - (columns - 1)/2)*cellSize,height,(row - (rows - 1)/2)*cellSize))
    let tileData = {
      kind: "tile",
      id: id,
      grid: {x: column, z: row},
      cellSize: cellSize
    }
    setUserData(tiles,id,tileData)
    setAttribute(tiles,"points","height",id,height)
  }
}
wrangle points in tiles as tile {
  # Higher tiles cost more to cross.
  tile.moveCost = 1 + tile.height/elevation
  tile.walkable = 1
  tile.color = mix(rgba(0.25,0.48,0.32,1),rgba(0.85,0.71,0.4,1),clamp(tile.height/elevation,0,1))
  if tile.height < waterLevel {
    tile.walkable = 0
    tile.color = rgba(0.15,0.39,0.58,1)
  }
  tile.scale = vector(cellSize*0.94,tile.height,cellSize*0.94)
}
inspect("Tiles",tiles)

# Choose "Tiles" to inspect the grid. Each tile has an ID and X/Z grid coordinates.
# A game can use walkable (1 = yes, 0 = no) and moveCost to plan movement.
# Track units and buildings by tile ID. Rebuild that list if the grid size changes.
# Try raising waterLevel to see which tiles become blocked.

# Build downward so each tile's position stays at the top of its column.
let columnMesh = box(1,1,1)
translate(columnMesh,vector(0,-0.5,0))
output("Terrain columns",instances(columnMesh,tiles))
let water = box(columns*cellSize,0.06,rows*cellSize)
translate(water,vector(0,waterLevel - 0.03,0))
color(water,rgba(0.1,0.33,0.48,0.65))
output("Water",water)
`
});
