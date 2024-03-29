
export enum Provider {
  DRIVE = 'GoogleDrive',
  FS = 'LocalFs'
}

export const providers = [
  {value: Provider.DRIVE, name: 'Google Drive'},
  {value: Provider.FS, name: 'Local Filesystem'},
];
