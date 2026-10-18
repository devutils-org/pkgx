complete -c pkgx -f
complete -c pkgx -n '__fish_use_subcommand' -a 'demo manage managed providers doctor list search info why deps install remove export diff plan apply audit clean history recipe tui completions help'
complete -c pkgx -n '__fish_seen_subcommand_from recipe' -a 'check verify install uninstall list forget'
complete -c pkgx -l provider -x -a 'apt pacman dnf flatpak brew'
complete -c pkgx -l json
complete -c pkgx -s f -l file -r -F
complete -c pkgx -s o -l output -r -F
complete -c pkgx -l state-dir -r -a '(__fish_complete_directories)'
complete -c pkgx -l profile -r
complete -c pkgx -l dry-run
complete -c pkgx -l yes
complete -c pkgx -s h -l help
complete -c pkgx -l save -r -F
complete -c pkgx -l plan -r -F
complete -c pkgx -l expires-in -x -a '300 600 1800 3600'
complete -c pkgx -l installed
complete -c pkgx -l limit -x
complete -c pkgx -l offset -x
complete -c pkgx -l allow-apply

complete -c pkgx -n '__fish_seen_subcommand_from demo' -l snapshot

complete -c pkgx -n '__fish_seen_subcommand_from manage' -l forget

complete -c pkgx -l sudo -d 'Delegate confirmed APT execution only'
