// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

export interface DiskDocument {
  text: string;
  revision: string;
}

/** Owns one tab's source and disk baseline. Frames borrow the current text. */
export class WorkspaceDocument {
  declare project: string;
  declare path: string;
  declare text: string;
  declare baseline: string;
  declare revision: string;
  declare conflict: DiskDocument | null;
  declare saving: boolean;

  /** @param project @param path @param disk */
  constructor(project: string, path: string, disk: DiskDocument) {
    this.project = project;
    this.path = path;
    this.text = disk.text;
    this.baseline = disk.text;
    this.revision = disk.revision;

    this.conflict = null;
    this.saving = false;
  }
  get dirty() {
    return this.text !== this.baseline;
  }
  /** Replace the local source without advancing its saved baseline. @param text */
  edit(text: string) {
    this.text = text;
  }
  /** Capture source and revision together so edits made during a save stay dirty. */
  snapshot() {
    return {text: this.text, revision: this.revision};
  }
  /** Advance the baseline to the submitted snapshot, preserving any newer local text.
   * @param snapshot @param disk
   */
  saved(snapshot: DiskDocument, disk: DiskDocument) {
    this.baseline = snapshot.text;
    this.revision = disk.revision;
    this.conflict = null;
  }
  /** Accept clean external changes or retain a conflict for explicit user resolution.
   * @param disk
   * @returns Whether the editor should reload.
   */
  external(disk: DiskDocument): boolean {
    if(disk.revision === this.revision || this.saving) {
      return false;
    }
    if(this.dirty) {
      this.conflict = disk;
      return false;
    }
    this.text = this.baseline = disk.text;
    this.revision = disk.revision;
    this.conflict = null;
    return true;
  }
  /** Acknowledge the competing revision while keeping the local edit buffer. */
  keepLocal() {
    if(this.conflict) {
      this.baseline = this.conflict.text;
      this.revision = this.conflict.revision;
      this.conflict = null;
    }
  }
  /** Discard local edits in favor of the retained conflicting disk snapshot. */
  reloadDisk() {
    if(this.conflict) {
      this.text = this.baseline = this.conflict.text;
      this.revision = this.conflict.revision;
      this.conflict = null;
    }
  }
}
