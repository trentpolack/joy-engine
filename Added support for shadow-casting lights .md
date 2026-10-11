Added support for shadow-casting lights (enabled by default) and properlly implemented PBR material support for FORM LAB's preview.
- Implemented luma.gl WebGPU shadows for directional and point lights, preserving all 32 supported point lights, masked-material casting, transparent lighting, and shadow diagnostics.
- Added reusable PBR environments with explicit resource ownership, plus FORM LAB studio reflections, editable preview lighting, and a Material studio example.
- Extended rendering regression checks, browser captures, and API documentation, and restored standalone FORM LAB font loading.
- Updated \`.gitignore\` to only ignore assets relevant to the project rather than the generic sweeping one.