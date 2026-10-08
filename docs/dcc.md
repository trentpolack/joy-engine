# DCC interchange
This folder contains importers for interchange files produced by digital-content-creation tools. Importers return portable engine data and do not own project catalogs, URLs, or loaded browser resources.

Audio does not belong here. `AudioBank.create()` fetches and decodes short effects into reusable Web Audio buffers. The bank owns its context and overlapping playback nodes; `destroy()` stops playback and closes the context. See [audio ownership and activation](audio.md).
