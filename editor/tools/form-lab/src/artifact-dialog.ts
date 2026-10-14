// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {download} from './document.ts';

export interface ArtifactFormat {
  id: string;
  label: string;
  description: string;
  mime: string;
}

interface SavePicker {
  showSaveFilePicker?: (options: {
    suggestedName: string;
    types: Array<{description: string; accept: Record<string, string[]>}>;
  }) => Promise<{createWritable(): Promise<{write(contents: string | ArrayBuffer): Promise<void>; close(): Promise<void>; abort?(): Promise<void>}>}>;
}

/**
 * Owns one save/export dialog. Content is captured before opening the OS picker;
 * cancellation never writes a file or silently substitutes a download.
 */
export class ArtifactDialog {
  private readonly dialog: HTMLDialogElement;
  private readonly form: HTMLFormElement;
  private readonly format: HTMLSelectElement;
  private readonly name: HTMLInputElement;
  private readonly destination: HTMLSelectElement;
  private readonly submit: HTMLButtonElement;
  private readonly listeners = new AbortController();
  private formats: readonly ArtifactFormat[] = [];
  private writing = false;
  private disposed = false;
  private opener: HTMLElement | null = null;
  private produce: (format: string) => string | ArrayBuffer = () => '';
  private completed: (format: string, contents: string | ArrayBuffer, saved: boolean) => void = () => {};

  constructor(dialog: HTMLDialogElement) {
    this.dialog = dialog;
    this.form = this.element<HTMLFormElement>('form');
    this.format = this.element<HTMLSelectElement>('#artifact-format');
    this.name = this.element<HTMLInputElement>('#artifact-name');
    this.destination = this.element<HTMLSelectElement>('#artifact-destination');
    this.submit = this.element<HTMLButtonElement>('#artifact-write');
    const signal = this.listeners.signal;
    this.form.addEventListener('submit', event => {
      event.preventDefault();
      void this.write();
    }, {signal});
    this.format.addEventListener('change', () => this.updateFormat(), {signal});
    this.destination.addEventListener('change', () => this.updateLocation(), {signal});
    for(const id of ['artifact-close', 'artifact-cancel']) {
      this.element(`#${id}`).addEventListener('click', () => {
        if(!this.writing) {
          dialog.close();
        }
      }, {signal});
    }
    dialog.addEventListener('cancel', event => {
      if(this.writing) {
        event.preventDefault();
      }
    }, {signal});
    dialog.addEventListener('close', () => {
      if(!this.disposed) {
        this.opener?.focus();
      }
    }, {signal});
  }

  /**
   * Open with an explicit set of formats; saving source never implicitly bakes overrides.
   */
  open(options: {
    title: string;
    name: string;
    formats: readonly ArtifactFormat[];
    opener: HTMLElement;
    produce: (format: string) => string | ArrayBuffer;
    completed?: (format: string, contents: string | ArrayBuffer, saved: boolean) => void;
  }) {
    if(this.dialog.open || this.disposed) {
      return;
    }
    this.formats = options.formats;
    this.opener = options.opener;
    this.produce = options.produce;
    this.completed = options.completed ?? (() => {});
    this.element('#artifact-title').textContent = options.title;
    this.submit.textContent = options.title;
    this.format.replaceChildren(...options.formats.map(format => new Option(format.label, format.id)));
    this.name.value = options.name;
    const pickerAvailable = typeof (window as Window & SavePicker).showSaveFilePicker === 'function';
    this.destination.options[0].disabled = !pickerAvailable;
    this.destination.value = pickerAvailable ? 'picker' : 'download';
    this.element('#artifact-destination-field').hidden = !pickerAvailable;
    this.element('#artifact-location-note').hidden = !pickerAvailable;
    this.element('#artifact-error').hidden = true;
    this.element('#export-note').hidden = options.formats[0].id === 'form';
    this.updateFormat();
    this.updateLocation();
    this.dialog.showModal();
  }

  /** Release owned listeners and close the dialog when its document editor is disposed. */
  destroy() {
    this.disposed = true;
    this.listeners.abort();
    this.dialog.close();
  }

  private element<T extends HTMLElement = HTMLElement>(selector: string): T {
    const element = this.dialog.querySelector<T>(selector);
    if(!element) {
      throw new Error(`Missing file dialog element ${selector}.`);
    }
    return element;
  }

  private selectedFormat(): ArtifactFormat {
    const format = this.formats.find(value => value.id === this.format.value);
    if(!format) {
      throw new Error('Choose a supported file format.');
    }
    return format;
  }

  private updateFormat() {
    const format = this.selectedFormat();
    const stem = this.name.value.replace(/\.(form|formlab|glb|obj|ply|json)$/i, '');
    this.name.value = `${stem}.${format.id}`;
    this.element('#artifact-description').textContent = format.description;
  }

  private updateLocation() {
    this.element('#artifact-location-note').textContent = this.destination.value === 'picker'
      ? 'Choose the folder and file name in the save dialog.'
      : 'Your browser controls the download folder. In Safari, choose “Ask for each download” in Settings → General → File download location to pick a folder each time.';
  }

  private async write() {
    if(this.writing) {
      return;
    }
    const errorLabel = this.element('#artifact-error');
    errorLabel.hidden = true;
    this.writing = true;
    this.submit.disabled = true;
    try {
      const format = this.selectedFormat();
      let name = this.name.value.trim();
      if(!name || /[/\\\0]/.test(name)) {
        throw new Error('Enter a file name without folder separators.');
      }
      if(!name.toLowerCase().endsWith(`.${format.id}`)) {
        name+= `.${format.id}`;
      }
      const contents = this.produce(format.id);
      const saved = this.destination.value === 'picker';
      if(saved) {
        const picker = window as Window & SavePicker;
        if(!picker.showSaveFilePicker) {
          throw new Error('Choose Browser downloads; this browser does not provide a save-location picker.');
        }
        const handle = await picker.showSaveFilePicker({
          suggestedName: name,
          types: [{description: format.label, accept: {[format.mime]: [`.${format.id}`]}}]
        });
        if(this.disposed) {
          return;
        }
        const stream = await handle.createWritable();
        try {
          await stream.write(contents);
          await stream.close();
        } catch(error) {
          // Aborting releases the writable and prevents publication of a partial file.
          await stream.abort?.().catch(() => {});
          throw error;
        }
      } else {
        download(name, contents, format.mime);
      }
      if(!this.disposed) {
        this.completed(format.id, contents, saved);
        this.dialog.close();
      }
    } catch(error) {
      if(!this.disposed && !(error instanceof DOMException && error.name === 'AbortError')) {
        errorLabel.hidden = false;
        errorLabel.textContent = error instanceof Error ? error.message : String(error);
      }
    } finally {
      this.writing = false;
      this.submit.disabled = false;
    }
  }
}
