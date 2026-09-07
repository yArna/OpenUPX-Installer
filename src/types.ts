export type Host = {
  app: string;
  label: string;
  minVersion?: string;
  icon: string;
};

export type PluginPackage = {
  path: string;
  fileName: string;
  fileSize: number;
  name: string;
  version: string;
  pluginId?: string;
  hosts: Host[];
  manifestVersion?: number;
  kind: "uxp" | "cep" | "xdx";
};

export type Environment = {
  platform: string;
  installerFound: boolean;
  installerPath?: string;
  creativeCloudFound: boolean;
};

export type InstallResult = {
  success: boolean;
  message: string;
  details?: string;
  canSideLoad?: boolean;
  activationPending?: boolean;
};

export type PermissionIssue = {
  path: string;
  message: string;
  hint: string;
};

export type SideloadPreflight = {
  supported: boolean;
  ready: boolean;
  pluginDirectory: string;
  registryPath: string;
  registryPaths: string[];
  hostLabels: string[];
  issues: PermissionIssue[];
  kind: "uxp" | "cep" | "xdx";
  debugModeEnabled: boolean;
  debugModeKeys: string[];
};
