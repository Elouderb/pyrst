import * as vscode from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
  Executable,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  // The language server launches an external executable (pyrst.server.path). To
  // avoid workspace-controlled code execution, only start it when the workspace
  // is TRUSTED. Syntax highlighting comes from the contributed grammar and works
  // regardless. If the user grants trust later, start the server then.
  if (vscode.workspace.isTrusted) {
    await startServer(context);
  } else {
    context.subscriptions.push(
      vscode.workspace.onDidGrantWorkspaceTrust(() => {
        void startServer(context);
      }),
    );
  }
}

async function startServer(context: vscode.ExtensionContext): Promise<void> {
  if (client) {
    return; // already started
  }
  const config = vscode.workspace.getConfiguration('pyrst');
  const serverPath: string = config.get<string>('server.path', 'pyrst');

  // The pyrst binary is an external native executable that speaks LSP over stdio.
  // We do NOT use TransportKind.ipc (Node.js IPC) — we use TransportKind.stdio
  // to talk to the Rust binary directly over stdin/stdout.
  const run: Executable = {
    command: serverPath,
    args: ['lsp'],
    transport: TransportKind.stdio,
  };

  const debug: Executable = {
    command: serverPath,
    args: ['lsp'],
    transport: TransportKind.stdio,
    // RUST_LOG can be set in the environment to enable server-side tracing.
    options: {
      env: {
        ...process.env,
        RUST_LOG: 'debug',
      },
    },
  };

  const serverOptions: ServerOptions = { run, debug };

  const clientOptions: LanguageClientOptions = {
    // Activate the client for .pyrs files opened as local files.
    documentSelector: [{ scheme: 'file', language: 'pyrst' }],
    // Relay the pyrst.trace.server setting to the LSP tracing infrastructure.
    traceOutputChannel: vscode.window.createOutputChannel('Pyrst Language Server Trace'),
  };

  client = new LanguageClient(
    'pyrst',
    'Pyrst Language Server',
    serverOptions,
    clientOptions,
  );

  // start() is async in vscode-languageclient v9+ and resolves once the
  // initialize/initialized handshake has completed.
  await client.start();

  context.subscriptions.push(client);
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
  }
}
