#!/usr/bin/env python3
"""
Context Engine CLI Startup Screen
Recreates Claude Code v2.0.14 welcome card style for Context Engine
"""

import os
import sys
from pathlib import Path

try:
    from rich.console import Console
    from rich.panel import Panel
    from rich.text import Text
    from rich.align import Align
    from rich.columns import Columns
    from rich import box
    from rich.style import Style
except ImportError:
    print("Error: rich library is required. Install with: pip install rich")
    sys.exit(1)

def create_welcome_screen():
    """Create the Context Engine welcome screen"""
    # Initialize console with legacy Windows support
    console = Console(legacy_windows=True, force_terminal=True)

    # Get current working directory
    current_dir = Path.cwd()

    # Create the C* NTXT ENGINE text to appear larger and bolder
    # We'll use a combination of padding and bold styling to simulate a larger size
    logo_text = Text()
    logo_text.append("\n", style=Style(color="#FF3B00", bold=True))
    # Add the logo with substantial padding to make it appear larger
    logo_text.append("    C✱ NTXT ENGINE    ", style=Style(color="#FF3B00", bold=True))
    logo_text.append("\n", style=Style(color="#FF3B00", bold=True))
    
    # Left panel content (logo and project info)
    left_content_parts = []
    
    # Add spacing to match Claude's layout
    left_content_parts.append("\n\n\n")
    
    # Add the logo with emphasis
    left_content_parts.append(logo_text)
    
    # Add welcome message and project info
    welcome_text = Text.from_markup(
        f"\n[bold white]Welcome back![/bold white]\n\n"
        f"[dim]Current Project:\n{current_dir}[/dim]"
    )
    left_content_parts.append(welcome_text)
    
    # Combine all parts
    left_content = Text.assemble(*left_content_parts)

    # Right panel content (tips and activity)
    right_content = Text.from_markup(
        "[bold]Tips for getting started[/bold]\n"
        "Run [cyan]/init[/cyan] to create CONTEXT.md with setup instructions\n\n"
        "[bold]Recent activity[/bold]\n"
        "No recent sessions"
    )

    # Create two panels
    panels = [
        Panel(left_content, box=box.ROUNDED, expand=True),
        Panel(right_content, box=box.ROUNDED, expand=True)
    ]

    # Create the main panel with both columns
    columns = Columns(panels, equal=True, expand=True)

    # Main welcome panel
    welcome_panel = Panel(
        columns,
        box=box.ROUNDED,
        border_style=Style(color="#FF3B00"),
        padding=(1, 2),
        expand=True
    )

    # Footer tagline
    footer = Align.center(
        Text("Context Engine - Compress the Chaos.", style=Style(color="#FF3B00", dim=True)),
        vertical="middle"
    )

    # Clear screen and display
    os.system('cls' if os.name == 'nt' else 'clear')
    console.print("\n")
    
    # Try to print with UTF-8 encoding
    try:
        console.print(welcome_panel)
    except (UnicodeEncodeError, UnicodeDecodeError):
        # Fallback: replace special characters and try again
        panel_str = str(welcome_panel).replace("✱", "*")
        console.print(panel_str)
    
    console.print("\n")
    console.print(footer)
    console.print("\n")

def main():
    """Main function to display the welcome screen"""
    try:
        create_welcome_screen()
    except KeyboardInterrupt:
        print("\nGoodbye!")
        sys.exit(0)
    except Exception as e:
        print(f"Error displaying welcome screen: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()