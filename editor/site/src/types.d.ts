interface Window {
  joyEditor?: {
    initialProject: {manifest: unknown; path: string; previewUrl: string} | null;
    openProject(): Promise<{manifest: unknown; path: string; previewUrl: string} | null>;
    popOutPreview(): Promise<void>;
  };
  joyDesktop?: {
    openBrowser?: (url:string)=>Promise<void>;
    chooseProject(): Promise<{id:string;name:string;preview:null;connected:boolean;external:boolean;path:string;capabilities:Array<{extensions:string[]}>} | null>;
  };
  showDirectoryPicker?: (options?: {mode?: string}) => Promise<FileSystemDirectoryHandle>;
}
