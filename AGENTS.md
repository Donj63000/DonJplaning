# AGENTS.md

## Mission du depot

Ce projet a pour objectif de construire un **logiciel de bureau Windows** permettant de creer, visualiser, modifier, valider et exporter des **plannings d'usine en 3x8**.

Le depot est actuellement un projet **Rust** minimal. Tant qu'une decision explicite differente n'est pas prise, je dois raisonner et construire le projet de facon **Rust-first**, avec une architecture propre, testable, robuste et maintenable.

Je travaille comme un vrai ingenieur logiciel :

- j'analyse le projet avant de proposer ou coder quoi que ce soit ;
- j'identifie precisement les fichiers, modules, risques et impacts ;
- je produis du code complet, propre, robuste et pense pour evoluer ;
- je vise la non-regression, la lisibilite, la testabilite et la maintenabilite ;
- je n'improvise pas de logique metier floue sur un sujet critique comme la planification d'usine.

## Maniere de travailler

Avant toute modification importante :

- je lis le code existant et je comprends reellement l'architecture en place ;
- je verifie la structure du projet, les dependances, les conventions, les modules et les points d'entree ;
- j'identifie ce qui doit etre modifie, ce qui doit etre ajoute, et les risques de regression ;
- si je propose un plan, il doit etre precis, robuste, realiste, decoupe proprement, avec impacts, risques, tests et verifications.

Quand je code :

- je privilegie une architecture claire avec separation entre **metier**, **cas d'usage**, **infrastructure** et **interface** ;
- je garde la logique metier du planning independante de l'UI autant que possible ;
- je prefere des types explicites, des structures metier fortes, des `enum` et des validations claires plutot que des chaines ou des booleens ambigus ;
- j'evite les `unwrap`, `expect`, raccourcis dangereux et comportements implicites non maitrises ;
- je traite correctement les erreurs avec des messages utiles ;
- je ne laisse pas de faux code "temporaire" silencieux, de TODO critiques non cadres, ni de logique cassante.

Quand j'ajoute des commentaires dans le code :

- les commentaires doivent etre **rares, utiles et en francais** ;
- je parle **a la premiere personne**, comme si j'expliquais mon intention dans le code ;
- exemple attendu : `// Ici je valide que le salarie ne recoit pas deux postes sur le meme creneau`.

## Vision produit

Le logiciel cible un usage operationnel pour une usine travaillant en **3x8**. Cela implique que je dois raisonner comme sur un produit metier serieux et non comme sur une simple demo.

Le logiciel devra a terme couvrir au minimum :

- la gestion d'une base d'utilisateurs representant les ouvriers de l'usine ;
- la gestion des salaries, equipes, postes et competences ;
- la definition des plages horaires de travail et des cycles de rotation ;
- la generation et l'edition de plannings journaliers, hebdomadaires et mensuels ;
- la generation d'un planning propre, lisible et exploitable visuellement ;
- la prise en compte des absences, conges, indisponibilites et remplacements ;
- la detection des conflits, sous-effectifs, sur-affectations et violations de regles ;
- la validation metier avant publication ;
- l'export et l'impression du planning ;
- la tracabilite des modifications manuelles importantes.

Base metier initiale du logiciel :

- le logiciel doit permettre de saisir et maintenir une base d'utilisateurs correspondant aux ouvriers de l'usine ;
- chaque ouvrier doit pouvoir etre rattache a un poste ;
- le logiciel doit pouvoir generer un planning clair avec un code couleur metier stable ;
- le logiciel doit avoir un theme professionnel soigne dans une ambiance Windows 11 ;
- la liste initiale des postes autorises est :
- `Operateur de production`
- `Operateur de salle blanche`
- `Chef d'equipes`
- `Autre`

Representation visuelle initiale du planning :

- les jours planifies doivent etre affiches sous forme de cases lisibles ;
- les cases **bleues** correspondent aux horaires de **nuit** : `21h00 - 05h00` ;
- les cases **rouges** correspondent aux horaires d'**apres-midi** : `13h00 - 21h00` ;
- les cases **jaunes** correspondent aux horaires du **matin** : `05h00 - 13h00` ;
- les cases **beiges** correspondent aux horaires de **journee** : `08h30 - 16h30` ;
- le code couleur doit rester coherent dans toute l'application, les exports et les vues de planning.

Direction visuelle initiale :

- le logiciel doit avoir un rendu **professionnel, soigne et moderne** ;
- l'ambiance visuelle cible est celle d'un **logiciel Windows 11** fluide, propre et lisible ;
- la palette principale doit suivre un theme **gris-vert** sobre et industriel ;
- l'interface doit inspirer la fiabilite, la clarte et le serieux d'un logiciel de production ;
- les animations, transitions et interactions doivent rester **fluides, discretes et utiles** ;
- l'interface doit eviter tout rendu brouillon, surcharge visuelle ou style amateur.

## Contraintes metier a respecter

Le domaine "planning d'usine en 3x8" est sensible. Je dois modeliser explicitement les regles plutot que les disperser dans l'interface.

Par defaut, je considere les regles suivantes comme structurantes :

- une journee de production est repartie sur **3 postes de 8 heures** ;
- les horaires exacts doivent etre **configurables** et non codes en dur ;
- le logiciel doit pouvoir generer un planning propre, lisible et comprehensible rapidement par un responsable d'usine ;
- chaque ouvrier doit exister dans une base utilisateur avant de pouvoir etre planifie ;
- chaque ouvrier doit pouvoir avoir un poste metier clairement defini ;
- les postes doivent etre modelises avec des types explicites et non comme du texte libre disperse partout dans l'application ;
- la liste initiale des postes metier a supporter est `Operateur de production`, `Operateur de salle blanche`, `Chef d'equipes` et `Autre` ;
- les types d'horaires initiaux a supporter sont `Nuit (21h00-05h00)`, `Apres-midi (13h00-21h00)`, `Matin (05h00-13h00)` et `Journee (08h30-16h30)` ;
- chaque type d'horaire doit avoir une couleur d'affichage stable associee : bleu pour nuit, rouge pour apres-midi, jaune pour matin, beige pour journee ;
- l'interface doit conserver une coherence visuelle professionnelle avec un theme gris-vert et une ambiance Windows 11 ;
- une equipe ou un salarie ne peut pas etre affecte a deux postes incompatibles sur le meme creneau ;
- les postes de nuit traversant minuit doivent etre geres proprement ;
- les regles de repos minimal entre deux prises de poste doivent etre prevues dans le modele ;
- les contraintes de qualification ou d'habilitation doivent pouvoir bloquer certaines affectations ;
- les absences, conges, formations, arrets et indisponibilites doivent etre integres au calcul ;
- les besoins minimum par poste ou secteur doivent etre verifiables ;
- les anomalies doivent etre remontees de maniere explicite ;
- les derogations ou corrections manuelles doivent rester tracables.

Je dois aussi anticiper les cas limites :

- passage d'un mois a l'autre ;
- passage d'une annee a l'autre ;
- annees bissextiles ;
- jours feries ;
- nuits a cheval sur deux dates ;
- changements d'heure si le perimetre fonctionnel doit les supporter ;
- rotation d'equipes sur plusieurs semaines.

## Architecture attendue

Tant que le produit est jeune, je dois d'abord consolider un **socle metier testable**. L'interface ne doit pas devenir le lieu principal de la logique de planification.

Je privilegie la structure suivante ou une variante equivalente :

- `domain` : entites, objets valeur, regles metier, validations, anomalies ;
- `application` : cas d'usage, orchestration, services de generation et de validation ;
- `infrastructure` : persistance, acces fichiers, base locale, import/export ;
- `ui` : interface bureau Windows, interactions utilisateur, affichage, navigation.

Regles d'architecture :

- toute regle critique de planning doit etre testable sans lancer l'interface ;
- l'UI ne doit pas contenir la logique metier profonde ;
- les formats d'entree et de sortie doivent etre isoles du coeur metier ;
- les dates, creneaux, rotations, postes et affectations doivent avoir des types explicites ;
- les postes metier de base doivent idealement etre portes par un `enum` ou un type metier dedie ;
- les types d'horaires et leur code couleur doivent idealement etre portes par un `enum` ou un type metier dedie ;
- le theme, la palette et les styles principaux doivent idealement etre centralises pour garantir la coherence visuelle ;
- les regles metier doivent etre parametrables des que cela evite un codage en dur fragile.

## Decisions techniques par defaut

Vu l'etat actuel du depot :

- je conserve **Rust** comme langage principal ;
- je fais d'abord emerger un coeur metier robuste avant de complexifier l'interface ;
- je privilegie des composants simples, testables et faiblement couples ;
- si une persistance locale est necessaire, je privilegie une solution robuste adaptee a un logiciel de bureau Windows, par exemple **SQLite**, sauf instruction contraire ;
- je ne bascule pas de stack UI lourdement sans analyse prealable et sans raison claire.

Si un choix technique important doit etre pose, je dois comparer proprement les options avant de figer la direction.

## Qualite de code

Chaque changement doit etre fait proprement :

- noms explicites ;
- fonctions courtes si possible ;
- invariants metier proteges par les types ou validations ;
- gestion d'erreur serieuse ;
- pas de duplication evitable ;
- pas de logique cachee dans l'interface ;
- pas de regression silencieuse ;
- pas de code inutilement complexe.

Je dois toujours chercher la solution la plus robuste et la plus professionnelle, pas juste la plus rapide a ecrire.

## Tests et verifications

A chaque tache, je dois terminer par une vraie verification de mon travail.

Regle obligatoire :

- a chaque fois que je code quelque chose ou que j'ajoute une fonctionnalite dans le logiciel, je dois ajouter au minimum **un test unitaire** qui couvre explicitement ce comportement ;
- je ne considere jamais un ajout comme termine s'il n'est pas couvert par des tests adaptes ;
- en plus du test unitaire, je dois ajouter des **tests complets** des que le comportement touche un flux metier, plusieurs composants, ou un scenario utilisateur important.

Obligations :

- tout ajout ou modification de comportement doit etre couvert par des tests ;
- ajouter des tests unitaires des que j'introduis ou modifie une regle metier ;
- ajouter des tests d'integration quand un flux complet ou plusieurs couches sont concernees ;
- couvrir les cas nominaux, les cas d'erreur et les cas limites metier ;
- executer les tests que j'ai ajoutes ;
- executer aussi les tests du projet pertinents pour detecter une regression ;
- verifier qu'aucune erreur de compilation ou de coherence evidente n'a ete introduite ;
- reverifier le code avant de considerer le travail termine.

Pour ce type de produit, je dois prioriser les tests autour de :

- generation de planning ;
- validation des affectations ;
- conflits de creneaux ;
- postes de nuit ;
- repos minimal ;
- absences et remplacements ;
- limites calendaires ;
- regles de couverture minimale.

## Definition de termine

Une tache n'est pas consideree comme terminee tant que :

- le besoin demande est reellement implemente ;
- le code est propre et coherent avec l'architecture ;
- chaque ajout de comportement est couvert au minimum par un test unitaire ;
- les tests complets necessaires existent ;
- les tests pertinents passent ;
- les risques ou limites restantes sont clairement signales ;
- je me suis assure que mon changement ne casse pas le reste.

## Ce que je dois eviter

- coder sans analyse prealable ;
- proposer un plan vague sans avoir lu le projet ;
- melanger logique metier et interface ;
- coder des regles metier critiques "en dur" sans possibilite d'evolution ;
- livrer du code non teste sur un sujet metier sensible ;
- corriger superficiellement un symptome sans traiter la cause ;
- ajouter des dependances sans justification ;
- faire des modifications larges sans verifier leurs effets.

## Priorite produit

Quand plusieurs directions sont possibles, je privilegie dans cet ordre :

1. la justesse metier du planning ;
2. la robustesse du coeur applicatif ;
3. la non-regression et la testabilite ;
4. la clarte de l'experience utilisateur ;
5. l'optimisation ou le raffinement secondaire.

## Regle finale

Je travaille comme si ce logiciel devait etre utilise en production dans une vraie usine. Chaque decision doit donc etre justifiee par la fiabilite, la clarte, la securite metier et la maintenabilite.
