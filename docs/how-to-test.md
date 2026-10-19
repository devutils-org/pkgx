# Tester xpac à la main

Parcours complet, du plus sûr au plus risqué. Chaque étape donne les commandes et ce
qu'il faut observer (« Attendu »). Cocher au fur et à mesure.

| Niveau | Où | Ce qui est testé |
| --- | --- | --- |
| A | n'importe où | démo, sans gestionnaire de paquets |
| B | ta machine (Arch, Mac…) | tout ce qui est **lecture seule** |
| C | ta machine, HOME temporaire | fichiers de config gérés (M8) |
| D | ta machine (Linux) | recettes : validation + sandbox |
| E | VM jetable (OrbStack Debian) | installations, suppressions, mode strict, nettoyage, TUI exécutant |

Règle : **rien qui modifie des paquets en dehors de la VM jetable** (niveau E).

## 0. Préparer

Compiler depuis le dépôt :

```sh
cargo build --release --locked -p xpac-cli
alias xpac="$PWD/target/release/xpac"
xpac --version            # xpac 0.1.0
```

Ou récupérer l'archive construite par la CI (macOS arm64, Linux gnu/musl) :

```sh
gh run download 39143553843 -n xpac-macOS-ARM64-host -D /tmp/xpac-dl
cd /tmp/xpac-dl && shasum -a 256 -c ./*.sha256 && tar -xzf xpac-*.tar.gz
```

Pour ne pas mélanger avec ton état réel, utiliser un dossier d'état dédié :

```sh
export XPAC_TEST=$(mktemp -d)
alias px="xpac --state-dir $XPAC_TEST/state"
```

Dans la suite, `P` désigne ton gestionnaire natif : `pacman` (Arch), `brew` (Mac),
`apt` (Debian/Ubuntu), `dnf` (Fedora).

## A. Démo (aucun prérequis)

```sh
xpac demo
xpac demo --snapshot --json
```

- [ ] bandeau DEMO, 5 paquets fictifs ;
- [ ] `/curl` filtre, Entrée ouvre la fiche, `i` infos, Échap revient ;
- [ ] Tab change de fournisseur, `?` affiche l'aide, `q` quitte ;
- [ ] `--snapshot --json` imprime les paquets sans ouvrir le TUI.

## B. Lecture seule sur ta machine

Aucune de ces commandes ne modifie le système ni ne demande root.

### Diagnostic et inventaire

```sh
xpac providers
xpac doctor
xpac list --provider P | head
xpac search ripgrep --provider P
xpac search rip --provider P --installed
```

- [ ] `providers` : ton gestionnaire est `available`, les autres expliquent pourquoi non ;
- [ ] `doctor` : pas d'erreur, journal vide ou lisible ;
- [ ] `list` : nom, version, raison d'installation (explicit/dependency/unknown).

### Provenance et dépendances

Prendre un paquet installé, par ex. `curl` (identité qualifiée : `pacman:curl:x86_64`,
`brew:curl:formula`, `apt:curl:amd64`) :

```sh
xpac info P:curl
xpac why <identité>
xpac deps <identité>
```

- [ ] `why` dit qui dépend du paquet ou s'il a été installé explicitement ;
- [ ] `deps` liste les expressions natives (alternatives, contraintes conservées).

### Prévisualisations (rien n'est exécuté)

Prendre un paquet **non installé** et présent dans tes dépôts : `pacman:cowsay`,
`brew:hello:formula`, `apt:hello`.

```sh
xpac install pacman:cowsay --dry-run
xpac plan pacman:cowsay
xpac audit --provider P
xpac clean --dry-run --provider P
```

- [ ] `install --dry-run` montre l'opération native et son risque, puis s'arrête
  (Pacman signale que son aperçu `--print` est une estimation) ;
- [ ] `audit` classe : cache sûr, orphelins suggérés, revue manuelle ;
- [ ] `clean --dry-run` ne supprime rien.

Sans `--yes` et sans terminal, une exécution est refusée :

```sh
xpac install pacman:cowsay </dev/null; echo $?   # 1, « noninteractive execution requires --yes »
```

### Manifestes, profils, export

`examples/profiles.toml` cible APT (Linux) et Homebrew (macOS). Sur Arch, un
fournisseur absent fait échouer `diff` (« cannot run apt-get ») : c'est voulu, il n'y a
pas de bascule implicite. Utiliser alors ce manifeste équivalent :

```sh
cat > $XPAC_TEST/profiles.toml <<'EOF'
version = 1
[packages.pacman]
install = ["curl"]
[profiles.tools.packages.pacman]
install = ["git"]
[profiles.dev]
extends = ["tools"]
[profiles.dev.packages.pacman]
install = ["cowsay"]
EOF
```

```sh
xpac export --provider P -o $XPAC_TEST/system.toml
xpac diff -f $XPAC_TEST/system.toml
xpac diff -f $XPAC_TEST/profiles.toml            # Mac/Debian : examples/profiles.toml
xpac diff -f $XPAC_TEST/profiles.toml --profile dev
xpac plan -f $XPAC_TEST/profiles.toml --profile dev
xpac diff -f $XPAC_TEST/profiles.toml --profile inconnu; echo $?
```

- [ ] l'export relu par `diff` donne tout `present`, rien de `missing` ;
- [ ] sans profil seul `curl` est demandé ; avec `dev`, `git` (hérité de `tools`) et
  `cowsay` s'ajoutent, et le plan propose d'installer ce qui manque ;
- [ ] un profil inconnu échoue (code 1) ;
- [ ] les paquets non déclarés sont listés « undeclared; retained », jamais supprimés.

### TUI en lecture seule

```sh
xpac tui --provider P -f $XPAC_TEST/system.toml
```

- [ ] j/k, PgUp/PgDn, Home/End naviguent ; `p` masque l'aperçu ;
- [ ] Entrée/`i` infos, `d` dépendances, `w` provenance, `o` audit du fournisseur ;
- [ ] `a` / `r` montrent un plan d'installation / suppression, **sans** l'exécuter ;
- [ ] `m` affiche la revue du manifeste ;
- [ ] `x` refuse : « enable --allow-apply » ;
- [ ] `R` recharge l'inventaire, `?` l'aide, `q` quitte et rend le terminal propre.

## C. Fichiers de config gérés (HOME temporaire)

Tout se passe dans un faux HOME : ton vrai `~/.tmux.conf` n'est jamais touché.
Toujours écrire les cibles entre quotes (`'~/.bashrc'`) pour que le shell ne les
remplace pas par ton vrai HOME.

```sh
F=$XPAC_TEST/files; mkdir -p $F/home $F/repo/dotfiles; cd $F/repo
fx() { HOME=$F/home xpac --state-dir $F/state "$@"; }

printf 'set -g mouse on\n' > dotfiles/tmux.conf
printf 'alias ll="ls -l"\n' > dotfiles/bashrc
printf 'ancien bashrc\n' > $F/home/.bashrc
cat > system.toml <<'EOF'
version = 1
[files."~/.config/tmux/tmux.conf"]
source = "dotfiles/tmux.conf"
mode = "0644"
[files."~/.bashrc"]
source = "dotfiles/bashrc"
EOF
```

1. État initial :

   ```sh
   fx diff -f system.toml
   fx plan -f system.toml; echo $?
   ```

   - [ ] `file missing ~/.config/tmux/tmux.conf` et `file unmanaged conflict ~/.bashrc` ;
   - [ ] `plan` échoue (code 1) en nommant `~/.bashrc (unmanaged_conflict)` ;
   - [ ] `ls -A $F` : pas de dossier `state` créé.

2. Adopter le fichier existant :

   ```sh
   fx file adopt '~/.bashrc' --dry-run
   fx file adopt '~/.bashrc'          # taper yes
   ls -l $F/state/files/backups
   ```

   - [ ] aperçu (taille, sha256, mode) puis confirmation ;
   - [ ] une sauvegarde 0600 contenant `ancien bashrc`.

3. Planifier et appliquer :

   ```sh
   fx plan -f system.toml
   fx apply -f system.toml            # taper yes
   ls -la $F/home $F/home/.config $F/home/.config/tmux
   fx apply -f system.toml
   ```

   - [ ] le plan montre un diff unifié (`-ancien bashrc` / `+alias ll…`, création du tmux.conf) ;
   - [ ] après apply : contenus attendus, `.bashrc` en 0600 (défaut), `tmux.conf` en 0644,
     `.config` et `.config/tmux` en 0700, aucun fichier `.xpac-*` ;
   - [ ] le second apply affiche « No file writes ».

4. Modification locale (drift) :

   ```sh
   echo '# perso' >> $F/home/.config/tmux/tmux.conf
   fx diff -f system.toml
   fx apply -f system.toml --yes; echo $?
   fx file list
   ```

   - [ ] `file drifted ~/.config/tmux/tmux.conf`, apply refusé, ta ligne conservée ;
   - [ ] `file list` : tmux.conf `drifted`, bashrc `intact`.

   Recopier la modification dans la source la remet en ordre :

   ```sh
   cp $F/home/.config/tmux/tmux.conf dotfiles/tmux.conf
   fx diff -f system.toml               # in sync
   ```

5. Pièges refusés :

   ```sh
   mkdir -p $F/ailleurs && echo secret > $F/ailleurs/victime
   ln -s $F/ailleurs/victime $F/home/.piege
   printf '[files."~/.piege"]\nsource = "dotfiles/bashrc"\n' >> system.toml
   fx diff -f system.toml
   fx apply -f system.toml --yes; echo $?
   cat $F/ailleurs/victime
   ```

   - [ ] `file unsupported ~/.piege (target is not a regular file)` ;
   - [ ] apply refusé, `victime` contient toujours `secret`.

   Retirer ensuite les deux dernières lignes de `system.toml` et le lien.

   Autres refus à essayer (chacun doit échouer avant toute écriture) :
   `source = "../x"`, une cible `"/etc/x"`, `mode = "0777"`, une source qui est un lien
   symbolique vers l'extérieur.

6. Suppression et oubli :

   ```sh
   fx file remove '~/.bashrc' --dry-run
   fx file remove '~/.bashrc'         # yes
   fx file forget '~/.config/tmux/tmux.conf' --yes
   ls -A $F/home $F/home/.config/tmux; fx file list
   ```

   - [ ] `.bashrc` supprimé (sauvegarde ajoutée dans `backups`) ;
   - [ ] `tmux.conf` toujours présent après `forget`, plus de reçu ;
   - [ ] modifier un fichier géré puis tenter `file remove` : refusé.

7. Verrou et TUI :

   Après `forget`, `tmux.conf` n'est plus géré : un `apply` refuserait
   (`unmanaged_conflict`). Le ré-adopter d'abord :

   ```sh
   fx file adopt '~/.config/tmux/tmux.conf' --yes
   fx apply -f system.toml --yes      # recrée .bashrc
   touch $F/state/files/lock && chmod 600 $F/state/files/lock
   echo x >> dotfiles/bashrc && fx apply -f system.toml --yes; echo $?
   rm $F/state/files/lock
   fx apply -f system.toml --yes
   HOME=$F/home xpac --state-dir $F/state tui --provider P -f system.toml   # puis m
   ```

   - [ ] avec le verrou : refus « file registry is locked », rien d'écrit ; une fois le
     verrou retiré, le même apply met `.bashrc` à jour ;
   - [ ] dans le TUI, `m` montre une section « Files (read-only here…) » avec l'état de
     chaque fichier ; le TUI n'écrit jamais de fichier.

## D. Recettes (scripts suivis)

```sh
xpac recipe check -f tests/fixtures/recipe.toml
```

- [ ] affiche la version épinglée, le sha256, l'interpréteur, les ressources ; aucun
  téléchargement.

Le cycle réel (téléchargement HTTPS, intégrité, installation, reçu, désinstallation,
`--observe`) tourne dans le sandbox fourni, sur Linux avec bubblewrap :

```sh
cargo build --locked -p xpac-cli
sh tests/sandbox/recipe-userns.sh target/debug/xpac
sh tests/sandbox/files-userns.sh "$PWD/target/debug/xpac"
```

- [ ] chaque script se termine par son message « passed ».

Pour l'écrire soi-même : un fichier `recipe.toml` comme dans
[docs/recipes.md](recipes.md), puis `xpac recipe verify`, `install --dry-run`,
`install`, `list`, `uninstall`. Uniquement avec un script de confiance, dans la VM.

## E. Transactions réelles (VM jetable)

Créer la machine comme dans [manual-validation.md §1](manual-validation.md)
(OrbStack Debian, xpac compilé et installé). Travailler avec l'utilisateur par défaut
(sudo sans mot de passe) sauf mention contraire. Supprimer la VM à la fin.

### Installer, supprimer, idempotence

```sh
xpac install apt:hello --dry-run
xpac install apt:hello --sudo          # yes
hello
xpac install apt:hello --sudo --yes    # déjà présent : rien à faire
xpac why apt:hello:amd64               # explicit (adapter l'architecture)
xpac remove apt:hello --sudo           # yes
xpac history --limit 5
```

- [ ] aperçu natif, confirmation, puis vérification après transaction ;
- [ ] répondre autre chose que `yes` annule sans rien changer ;
- [ ] `history` montre chaque transaction et son résultat.

### Manifeste

```sh
cat > ~/m.toml <<'EOF'
version = 1
[packages.apt]
install = ["hello", "cowsay"]
EOF
xpac diff -f ~/m.toml
xpac apply -f ~/m.toml --dry-run
xpac apply -f ~/m.toml --sudo          # yes
xpac apply -f ~/m.toml --sudo --yes    # no-op
```

- [ ] les paquets non déclarés restent installés (mode additif) ;
- [ ] le second apply ne lance aucune transaction.

### Plans sauvegardés

```sh
xpac remove apt:hello --sudo --yes
xpac plan apt:hello --save ~/p.json --expires-in 300
xpac apply --plan ~/p.json --dry-run
xpac apply --plan ~/p.json --sudo      # yes
xpac plan apt:cowsay --remove --save ~/old.json --expires-in 1; sleep 2
xpac apply --plan ~/old.json --sudo --yes; echo $?
```

- [ ] le plan valide s'applique ;
- [ ] le plan expiré est refusé (code 1) ;
- [ ] installer/supprimer un paquet entre `plan --save` et `apply --plan` : refus « stale ».

### Mode strict

```sh
xpac manage apt:hello:amd64 --dry-run
xpac manage apt:hello:amd64            # yes
xpac managed
xpac diff -f ~/xpac/examples/strict.toml
xpac apply -f ~/xpac/examples/strict.toml --sudo   # yes
xpac managed
```

- [ ] seul `hello` (autorisé) est proposé à la suppression, rien d'autre ;
- [ ] `cowsay`, non autorisé, reste installé ;
- [ ] `manage … --forget` retire une autorisation sans rien désinstaller.

### Audit et nettoyage

```sh
xpac remove apt:cowsay --sudo --yes    # ses dépendances deviennent orphelines
xpac audit
xpac clean --dry-run
xpac clean --orphans --sudo            # yes
xpac clean --cache --sudo              # yes
```

- [ ] `audit` liste les orphelins et le cache mesuré ;
- [ ] `clean --orphans` passe par un plan revu et ne retire que les orphelins ;
- [ ] `clean --cache` lance seulement `apt-get clean`.

### TUI exécutant

```sh
xpac tui --allow-apply --sudo -f ~/m.toml
```

- [ ] `m` puis `x` : il faut taper `apply` pour exécuter ; Échap annule ;
- [ ] sur un paquet, `r` puis `x` + `apply` le supprime ; la progression s'affiche et
  Ctrl-C annule proprement ;
- [ ] pour la saisie d'un vrai mot de passe sudo, suivre
  [manual-validation.md §1](manual-validation.md) (utilisateur `testeur`).

### Fichiers gérés en vrai

Refaire la partie C dans la VM avec le vrai HOME et un manifeste qui combine
`[packages.apt] install = ["tmux"]` et `[files."~/.tmux.conf"]` : apply installe
tmux, puis écrit le fichier après une seconde confirmation.

## Non couvert ici

Flatpak et DNF/Pacman en exécution réelle passent par les racines jetables de
`tests/sandbox/` et la CI ; Homebrew en exécution réelle par une VM macOS
([manual-validation.md §3](manual-validation.md)). Ne jamais lancer ces transactions
sur ta machine de tous les jours.
