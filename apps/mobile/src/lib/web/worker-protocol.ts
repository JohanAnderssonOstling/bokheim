export interface WorkerCommandRequest {
  kind: "command";
  id: number;
  command: string;
  args: unknown[];
}

export type WorkerCommandResponse =
  | { kind: "response"; id: number; ok: true; value: unknown }
  | { kind: "response"; id: number; ok: false; error: string };

export function toWorkerCommandName(command: string): string {
  return command.replace(/([a-z\d])([A-Z])/g, "$1_$2").toLocaleLowerCase();
}
