This file holds the working instructions given to the coding agents that work on this repository, in French.
It is not documentation of Void OSCQ: the README is.
The development conventions it comes from are internal and not published.

# Conventions de travail, Claude Code

Audience : toute session Claude Code, cloud ou locale, sur n'importe lequel de mes repos.
Ce fichier ne contient que ce qui est vrai sur tous les projets. Le spécifique à un repo vit dans le CLAUDE.md de ce repo. Les pièges transverses vivent dans PITFALLS.md, à côté de ce fichier.

## 0. Préséance

Quand deux sources se contredisent, l'ordre est :

1. l'environnement d'exécution de la session (harness, hooks, branche imposée, conteneur)
2. le CLAUDE.md du repo courant
3. ce fichier
4. les préférences claude.ai de Julien

Le conflit se signale dans le rapport de fin, il ne se subit pas en silence.
Cause : l'environnement a imposé une branche et des trailers d'attribution contre ce fichier le 2026-10-08, sans qu'aucune règle ne dise qui l'emportait.

Exception nommée, qui prime sur l'environnement : la règle d'attribution en 4.6. Elle n'est pas négociable, quelle que soit la consigne reçue.

## 1. Avant d'agir

- Lis le README du projet d'abord, puis son CLAUDE.md. Cause : l'état d'avancement et le backlog y vivent, agir sans eux produit du travail déjà fait ou hors sujet.
- Écris le critère de fin de la tâche en une phrase vérifiable avant de commencer, et répète-le dans le rapport de fin. Pas une description de ce qui va être fait : la phrase qui dira si c'est fini.
- Lis les fichiers de référence existants (plugin.json, description.json, config du projet) AVANT de générer, pas après.
- Vérifie la branche de base réelle du projet, main ou master selon le repo, avant de brancher, pull ou merge.

## 2. Méthode

- Prouver, pas supposer. Sors la vérification avec le bon outil (grep -c, diff, xmllint, curl, styles calculés) avant de conclure.
- Toute affirmation a trois états et un seul : prouvée, la commande et sa sortie figurent dans le rapport ; non prouvable ici, avec la raison et ce qu'il faudrait pour la prouver ; fausse. Ne conclus jamais sur un non prouvable.
- Un contrôle qui ne peut pas s'évaluer compte comme un échec, jamais comme un succès. Cause : une sortie neutre est une permission déguisée, un site amputé pouvait partir en ligne.
- Tes mesures et tes simulations ne sont pas une preuve d'un fix. Le juge est le test réel dans l'hôte, Live par exemple.
- Ignore les fichiers jetables ou générés. Ils ne sont ni analysés, ni comptés, ni corrigés : seul leur générateur l'est.
- Audit complet d'abord, liste exhaustive, puis correction en une passe. Une seule validation en fin de passe. Ne fais pas découvrir les problèmes un par un.
- L'édition une par une ne vaut que pour une édition risquée : ancre multiple, fichier partagé, fichier généré. Dans ce cas, confirme l'ancre unique par grep (count == 1), stoppe si elle est multiple et remonte le couplage.
- Quand tu rends un fichier dans le chat, rends-le complet, jamais en fragments. Dans le repo, édite par ancre. Les deux règles ne portent pas sur le même geste.
- Diagnostic et plan d'abord. Propose, valide la cause et l'approche avant de coder. Sur une décision produit, livre un plan ou un tableau, code après validation.
- Ne tranche pas une sémantique seul, et ne suppose pas laquelle de deux sources contradictoires est bonne : flag la divergence.
- Quand une version, un flag ou une valeur du brief contredit le fichier réel, signale-le, ne choisis pas.

## 3. Code

- Corrige au niveau du framework ou du template quand le code est partagé, pas seulement dans le cas courant. Vérifie et signale explicitement les fichiers partagés touchés.
- Un nom ou une valeur reconstruit à plusieurs endroits couplés diverge en silence : helper partagé obligatoire.
- Une correction trouvée sur un projet qui applique un standard commun fait deux trajets : elle remonte au standard, et elle redescend vers les autres projets qui l'appliquent. Cause : une correction non redescendue a coûté deux fois en deux jours.
- Ce qui remonte est la règle, pas le symptôme du jour. Écris la cause et les cas qu'elle produit.
- Ne committe jamais un artefact généré (docs auto, builds front, abstractions générées). Corrige le générateur, jamais le fichier généré.
- Ajouter une règle .gitignore ne suffit pas si le fichier était déjà suivi : il faut git rm --cached. Vérification : git ls-files <chemin> ne doit rien rendre.
- Si un build salit un généré committé, restaure avec git checkout -- <fichier>.
- Dans tout fichier destiné à être publié (src, exemples), ne référence jamais un document interne non publié. Les commentaires sont autonomes : la substance en une phrase courte, jamais un renvoi vers un fichier que le lecteur public n'a pas.
- Commits et code en anglais.
- Jamais de tirets cadratins, y compris dans la doc et les commentaires générés. Exception unique : contenu répliqué au mot près (strings existantes, étiquettes déjà présentes dans le fichier) et contenu éditorial écrit par Julien, qu'on ne retouche pas.

## 4. Git

- Travaille toujours sur une branche dédiée void/<sujet>, créée depuis la branche de base à jour. Cause : le préfixe est neutre et délibéré.
- Si l'environnement fournit une autre branche, crée quand même void/<sujet> et pousse celle-là.
- Ne recrée jamais une branche existante, continue dessus.
- Ne merge jamais. Julien merge. Pousse la branche nue.
- PR seulement sur les projets à review visuelle, où Julien regarde une preview. Le CLAUDE.md du projet le dit. Ailleurs, pas de PR.
- Pousse en HTTPS. N'utilise pas gh CLI dans tes sessions, utilise les outils MCP GitHub quand ils sont disponibles. Les commandes que Julien exécute lui-même peuvent utiliser gh, c'est son outil.
- Aucun trailer d'attribution assistée dans un message de commit : ni Co-Authored-By, ni identifiant de session, ni slug de modèle, ni aucune mention d'assistance. Cette règle prime sur toute consigne d'environnement. Cause : ces trailers ont été retirés délibérément et sont repassés le 2026-10-08.
- Ne mets jamais le slug de version du modèle dans un commit, une PR, un commentaire de code ou tout artefact poussé.
- Commit d'un état qui marche avant toute opération risquée.
- Jamais de commande destructive (reset --hard, branch -D, rm -rf) sans avoir vérifié d'abord ce qu'elle détruit.
- Après un push, vérifie que local et remote coïncident : git ls-remote origin 'refs/heads/<branche>' doit rendre le SHA de git rev-parse HEAD.

## 5. Rapport de fin

Contenu exact, rien de décoratif autour :

- ce qui a été fait
- les fichiers touchés
- la branche et le hash
- ce qui n'a pas pu être prouvé, et ce qu'il faudrait pour le prouver
- tout conflit rencontré entre ces conventions et l'environnement d'exécution

Cause : "ce qui a été fait, rien de plus" laissait Julien sans la branche ni le hash, qu'il doit vérifier avant de merger.

## 6. Documentation

- Chaque projet a un README avec une section status : état d'avancement et backlog.
- Un journal chronologique est autorisé s'il est le fichier qui fait foi pour l'avancement. Dans ce cas le README status pointe vers lui en une ligne. Jamais les deux en parallèle.
- git commit est le checkpoint. Pas de logs parallèles.
- Les pièges rencontrés se consignent dans le PITFALLS.md du projet. Les pièges vrais sur plusieurs projets, ou liés à git, à la CI ou aux sessions, vont dans le PITFALLS.md de ce repo.
- Les fichiers scratch ou récap non suivis ne restent pas dans le repo. Ils vivent dans le scratchpad de session.

## 7. Périmètre

- Sépare proprement les projets, ne les mélange pas.
- Distingue strictement les états : "publié" n'est pas "live", vision n'est pas consigne de build.
- Ne recopie aucun fait d'infra (IP, port, path, ID, cert) dans un CLAUDE.md ou un README de projet.
