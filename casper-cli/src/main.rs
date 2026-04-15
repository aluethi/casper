mod commands;
mod credentials;
mod http;

use clap::{Parser, Subcommand};

/// Casper CLI — manage agents, deployments, knowledge, and more.
#[derive(Parser)]
#[command(name = "casper", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Authentication: login, logout, status.
    Auth {
        #[command(subcommand)]
        cmd: commands::auth::AuthCmd,
    },

    /// Run an agent with a message.
    Run {
        /// Agent name.
        agent: String,
        /// Message to send to the agent.
        message: String,
    },

    /// Manage agents: list, get, create, delete, export, import.
    Agent {
        #[command(subcommand)]
        cmd: commands::agent::AgentCmd,
    },

    /// Manage deployments: list, create, test.
    Deploy {
        #[command(subcommand)]
        cmd: commands::deploy::DeployCmd,
    },

    /// Manage the knowledge base: upload, list, search.
    Knowledge {
        #[command(subcommand)]
        cmd: commands::knowledge::KnowledgeCmd,
    },

    /// Manage agent memory: show, update.
    Memory {
        #[command(subcommand)]
        cmd: commands::memory::MemoryCmd,
    },

    /// Manage tenant-level memory: show, update.
    TenantMemory {
        #[command(subcommand)]
        cmd: commands::memory::TenantMemoryCmd,
    },

    /// Manage API keys: create, list, revoke.
    Keys {
        #[command(subcommand)]
        cmd: commands::keys::KeysCmd,
    },

    /// Manage secrets: set, list, delete.
    Secrets {
        #[command(subcommand)]
        cmd: commands::secrets::SecretsCmd,
    },

    /// List conversations.
    Conversations {
        #[command(subcommand)]
        cmd: ConversationsCmd,
    },

    /// Usage information.
    Usage {
        #[command(subcommand)]
        cmd: UsageCmd,
    },

    /// Audit log.
    Audit {
        #[command(subcommand)]
        cmd: AuditCmd,
    },

    /// Snippets.
    Snippets {
        #[command(subcommand)]
        cmd: SnippetsCmd,
    },
}

#[derive(Subcommand)]
enum ConversationsCmd {
    /// List recent conversations.
    List,
}

#[derive(Subcommand)]
enum UsageCmd {
    /// Show usage summary.
    Summary,
}

#[derive(Subcommand)]
enum AuditCmd {
    /// Query the audit log.
    Query,
}

#[derive(Subcommand)]
enum SnippetsCmd {
    /// List snippets.
    List,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Auth { cmd } => commands::auth::handle(cmd).await,
        Commands::Run { agent, message } => commands::run::handle(&agent, &message).await,
        Commands::Agent { cmd } => commands::agent::handle(cmd).await,
        Commands::Deploy { cmd } => commands::deploy::handle(cmd).await,
        Commands::Knowledge { cmd } => commands::knowledge::handle(cmd).await,
        Commands::Memory { cmd } => commands::memory::handle(cmd).await,
        Commands::TenantMemory { cmd } => commands::memory::handle_tenant(cmd).await,
        Commands::Keys { cmd } => commands::keys::handle(cmd).await,
        Commands::Secrets { cmd } => commands::secrets::handle(cmd).await,
        Commands::Conversations { cmd } => match cmd {
            ConversationsCmd::List => commands::conversations::handle_list().await,
        },
        Commands::Usage { cmd } => match cmd {
            UsageCmd::Summary => commands::usage::handle_summary().await,
        },
        Commands::Audit { cmd } => match cmd {
            AuditCmd::Query => commands::audit::handle_query().await,
        },
        Commands::Snippets { cmd } => match cmd {
            SnippetsCmd::List => commands::snippets::handle_list().await,
        },
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
