complete -c xpac -f
complete -c xpac -n '__fish_use_subcommand' -a 'demo manage managed providers doctor list search info why deps install remove export diff plan apply audit clean history file recipe tui completions help'
complete -c xpac -n '__fish_seen_subcommand_from file' -a 'adopt list forget remove'
complete -c xpac -n '__fish_seen_subcommand_from recipe' -a 'check verify install uninstall list forget'
complete -c xpac -l provider -x -a 'apt pacman dnf flatpak brew'
complete -c xpac -l json
complete -c xpac -s f -l file -r -F
complete -c xpac -s o -l output -r -F
complete -c xpac -l state-dir -r -a '(__fish_complete_directories)'
complete -c xpac -l profile -r
complete -c xpac -l dry-run
complete -c xpac -l yes
complete -c xpac -s h -l help
complete -c xpac -l save -r -F
complete -c xpac -l plan -r -F
complete -c xpac -l expires-in -x -a '300 600 1800 3600'
complete -c xpac -l installed
complete -c xpac -l limit -x
complete -c xpac -l offset -x
complete -c xpac -l allow-apply

complete -c xpac -n '__fish_seen_subcommand_from demo' -l snapshot

complete -c xpac -n '__fish_seen_subcommand_from manage' -l forget

complete -c xpac -l sudo -d 'Delegate confirmed APT execution only'
