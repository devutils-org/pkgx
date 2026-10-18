# Source this file explicitly to enable Bash completions.
_pkgx() {
    local cur prev command
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"
    command="${COMP_WORDS[1]}"
    case "$prev" in
        --provider) COMPREPLY=( $(compgen -W 'apt pacman dnf flatpak brew' -- "$cur") ); return ;;
        -f|--file|-o|--output|--plan|--save) COMPREPLY=( $(compgen -f -- "$cur") ); return ;;
        --expires-in) COMPREPLY=( $(compgen -W '300 600 1800 3600' -- "$cur") ); return ;;
        --state-dir) COMPREPLY=( $(compgen -d -- "$cur") ); return ;;
    esac
    if [[ "$command" == file && $COMP_CWORD -eq 2 ]]; then
        COMPREPLY=( $(compgen -W 'adopt list forget remove' -- "$cur") )
    elif [[ "$command" == recipe && $COMP_CWORD -eq 2 ]]; then
        COMPREPLY=( $(compgen -W 'check verify install uninstall list forget' -- "$cur") )
    elif (( COMP_CWORD == 1 )); then
        COMPREPLY=( $(compgen -W 'demo manage managed providers doctor list search info why deps install remove export diff plan apply audit clean history file recipe tui completions help' -- "$cur") )
    else
        COMPREPLY=( $(compgen -W '--help --json --sudo --provider --file --output --profile --state-dir --dry-run --yes --installed --forget --allow-apply --snapshot --limit --offset --save --expires-in --plan' -- "$cur") )
    fi
}
complete -F _pkgx pkgx
