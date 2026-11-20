# Plan d’implémentation du frontend SoundTime

**État au 19 novembre 2026.** Les lots 1 à 6 sont intégrés dans la copie de travail issue de `e660472`. Un membre peut suivre un lien privé vers son album, se connecter et écouter le fichier en continu pendant la navigation. Les images Linux ARM64 du lot 7 ont passé la recette locale sur instance privée. La promotion publique reste conditionnée à la CI.

Avant les corrections, les 593 tests unitaires passaient, alors que Chromium recevait un HTTP 401 sur le flux privé après une connexion valide. Les neuf scénarios de recette exercent désormais le navigateur, le proxy, le serveur et PostgreSQL ensemble.

## L’interface à prendre comme référence

| Surface | Comportement à conserver lors de l’intégration |
| --- | --- |
| Navigation | Sur ordinateur, sidebar gris clair avec icônes et libellés, destination active grisée, compte en bas et recherche dans l’en-tête. À partir de 768 px, on peut replier la sidebar. En dessous, quatre onglets et un menu donnent accès aux mêmes destinations autorisées. |
| Accueil et catalogue | Fond blanc, titres alignés à gauche, onglets soulignés, filtre local distinct de la recherche globale. Les pochettes apportent la couleur. À grande largeur, la vue grille affiche quatre cartes bordées. Les vues liste et grille donnent accès aux mêmes morceaux. |
| Connexion | Page indépendante avec carte centrée, formulaire à gauche, métal bleu animé à droite. Sur téléphone, le formulaire précède l’illustration. Le survol déforme les reflets, la sortie les laisse revenir au repos. La préférence de mouvement réduit affiche le poster. Les apparences clair, sombre et système appartiennent à cet écran. |
| Lecture | Le bandeau compact réserve son espace en bas du contenu, au-dessus des onglets sur mobile. Le grand lecteur, la file et les paroles utilisent la même piste et la même position. Les pochettes manquantes et les titres longs ont un rendu prévu. |

Le socle est **Svelte 5, SvelteKit 3, TypeScript, Tailwind 3 et shadcn-svelte**. Les boutons et champs partagés existent dans `components/ui`. Les onglets et dialogues s’appuient déjà sur Bits UI. Réutiliser ces composants, les variables de thème et les composants métier du catalogue. Les routes assemblent les vues, le client API porte les contrats HTTP et les stores existants gardent l’état audio. Les skills Svelte et le MCP servent à vérifier les composants modifiés pendant le développement. [Configuration shadcn-svelte](../frontend/components.json) · [Styles et thèmes](../frontend/src/app.css) · [Assemblage des écrans](../frontend/src/routes/+layout.svelte) · [Composants catalogue](../frontend/src/lib/components/catalog)

## Lots à intégrer

| Lot et dépendances | Livrable | Preuve de fin |
| --- | --- | --- |
| ✓ **1 · Établir la référence** — premier changement | Isoler les fichiers de la refonte des travaux parallèles. Sur une instance de test jetable, rendre l’initialisation Cypress reproductible avec le jeton de création admin et le champ `p2p_enabled`. Préparer deux membres, un fichier audio lisible, un album et des playlists publique et privée. Capturer l’accueil, le lecteur et la connexion avec ces données. | Un clone propre permet de lancer le scénario deux fois. Le test échoue explicitement si le morceau manque. Les captures de référence couvrent 390 et 1440 px avec une image fixe pour l’animation. |
| ✓ **2 · Accès à l’instance** — après 1 | Aligner setup, politique d’inscription, page de compte et redirection. Distinguer un statut d’instance indisponible d’un statut public confirmé. Conserver le lien interne demandé avant connexion. En cas de panne, proposer le réessai et garder la démo locale identifiable. | Tester instance neuve, publique ouverte, publique fermée aux inscriptions, privée, puis indisponible. Un lien d’album protégé ramène à cet album après connexion. Un invité reçoit uniquement les actions autorisées. |
| ✓ **3 · Lecture réelle** — après 1, en parallèle de 2 | Rendre le flux privé lisible par l’élément audio natif, puis exposer l’échec et le réessai dans le lecteur. Vérifier file, piste suivante, radio et commandes système sur le moteur existant. | Sur un fichier servi par le backend, la position avance, pause la stabilise et la recherche temporelle reprend au point choisi. Changer de page ou ouvrir le grand lecteur conserve piste, position et file. Un invité se voit refuser le flux privé. |
| ✓ **4 · Bibliothèque et playlists** — après 1, en parallèle de 2 et 3 | Donner à la bibliothèque et au sélecteur « Ajouter à une playlist » une source propriétaire paginée. Conserver la liste publique pour la découverte. Brancher pagination, mutations et actualisation des listes sur les mêmes contrats. | Le membre A crée une playlist privée, y ajoute un titre et la retrouve après rechargement. B ne peut ni la lire ni la modifier. Le sélecteur atteint la 21e playlist et la bibliothèque le 101e import. Un échec d’ajout permet une nouvelle tentative sans doublon. |
| ✓ **5 · Étendre le design au catalogue** — après 1, intégration avec 3 et 4 | Harmoniser Explorer, résultats de recherche, pistes, albums, artistes, playlists et leurs fiches avec les composants d’accueil. Appliquer les mêmes règles à la bibliothèque, aux favoris, à l’historique et à l’import. Préserver les URL et les commandes existantes. | Du clavier ou du téléphone, chercher un artiste, ouvrir son album, lancer un titre, l’ajouter aux favoris puis le retrouver. Les états chargement, vide, panne partielle, refus et réessai sont distincts. Le bouton Retour restitue un contexte de navigation cohérent. |
| ✓ **6 · Paramètres et exploitation** — après 2 et les composants communs de 5 | Extraire les sections compte et lecture de Paramètres, puis les domaines d’Administration au fil de leur migration, en commençant par le stockage et ses tests. Harmoniser setup et conditions d’utilisation. Vérifier que chaque réglage audio affiché a un effet observable. | Un membre change ses préférences et se déconnecte. Un administrateur retrouve les réglages, le suivi des tâches, les thèmes et les plugins. L’annulation d’une confirmation rend le focus au déclencheur. Les droits sont vérifiés lors d’un accès direct à l’URL. |
| Recette locale validée · **7 · Qualifier et publier** — après 2 à 6 | Brancher le parcours Cypress fiable dans la CI, essayer l’image exacte derrière Nginx, puis répéter le retour arrière. | La matrice ci-dessous passe sur la version candidate. L’équipe dispose de la révision, du digest, des résultats et du digest précédent testé. |

### Suivi du catalogue

- **Navigation sur réseau lent — validée sur le frontend candidat.** À 1440 et 390 px, les nouveautés d’Explorer apparaissent ensemble dans les deux ordres d’arrivée des réponses. L’accueil passe du gabarit de chargement aux quatre titres sélectionnés. Une section en erreur ou sans réponse laisse accéder aux morceaux disponibles et propose de réessayer. Les retours navigateur conservent la destination et la lecture en cours. Le parcours entre fiches a aussi été vérifié avec WebKit mobile en mode headless.
- **Titres sans crédit d’artiste — validé sur l’instance privée locale.** L’annuaire affiche 10 artistes crédités. Les 15 titres sans artiste font toujours partie des 25 morceaux de la bibliothèque. La recherche, la pagination de l’annuaire et la lecture d’un titre sans crédit ont passé la recette derrière Nginx.

**Ordre de livraison.** Une fois les lots 2 à 4 réunis, la première version de démonstration utilise le vrai backend et des comptes ordinaires. Le lot 5 peut avancer dès le lot 1 sur les composants visuels. La validation d’une action métier attend le contrat correspondant. L’administration peut être découpée en plusieurs revues sans immobiliser le parcours d’écoute. [Scénarios Cypress existants](../frontend/cypress/e2e) · [Initialisation des tests](../frontend/cypress/support/e2e.ts) · [Administration](../frontend/src/routes/admin/+page.svelte)

**Découpage du code.** Pour le lot 5, garder les en-têtes, grilles, listes et états du catalogue dans `components/catalog`, les primitives visuelles sans connaissance de l’API dans `components/ui`. Réutiliser `catalogQuery` pour que la réponse d’une recherche lente ne remplace pas la page courante, et l’invalidation des playlists après une mutation réussie. Pour le lot 6, extraire un domaine accompagné de ses tests à chaque revue. La page admin actuelle concentre plus de 3 300 lignes. Le stockage offre un premier découpage vérifiable grâce à ses tests existants. [Requêtes du catalogue](../frontend/src/lib/catalog/query.svelte.ts) · [Invalidation des playlists](../frontend/src/lib/catalog/playlists.svelte.ts) · [Tests du stockage](../frontend/src/routes/admin/storage.test.ts)

## Décisions d’implémentation

**Explorer.** Le champ de recherche mène aux résultats du catalogue. L’onglet Nouveautés présente jusqu’à quatre albums et six titres récemment ajoutés. Tendances, Artistes, Genres et Playlists chargent leurs données à l’ouverture de la rubrique. Le paramètre d’URL `tab` conserve la rubrique au rechargement et dans l’historique du navigateur. La recherche élargie au réseau reste une action explicite sur la page de résultats. [Page Explorer](../frontend/src/routes/explore/+page.svelte) · [Tests de navigation](../frontend/src/routes/explore/page.test.ts)

1. **Autorisation des médias privés.** Le navigateur échange le Bearer contre un cookie HttpOnly limité aux chemins de médias. Sa validité est plafonnée à dix minutes et à la durée restante du Bearer. Le lecteur utilise l’URL du morceau, renouvelle l’autorisation à la reprise et conserve la position. La déconnexion arrête le lecteur et expire le cookie. [Contrat HTTP](api-reference.md) · [Autorisation média](../backend/crates/soundtime-server/src/auth/media.rs)
2. **Politique d’inscription.** Le setup, le formulaire, l’annuaire et le serveur partagent la politique effective. Les réglages absents sur les installations antérieures conservent leur comportement. Un lien privé récupère son chemin local après connexion. [Politique serveur](../backend/crates/soundtime-server/src/auth/registration.rs) · [Navigation](../frontend/src/lib/access/navigation.ts)
3. **Bibliothèque personnelle.** La bibliothèque et le sélecteur de playlist utilisent `GET /api/me/playlists`. La découverte conserve sa collection publique. Les changements de favoris et de playlists actualisent les vues concernées. [API playlists](../backend/crates/soundtime-server/src/api/playlists.rs) · [Bibliothèque](../frontend/src/routes/library/+page.svelte)

## Recette de la version candidate

| Axe | Contrôle reproductible |
| --- | --- |
| Fidélité et navigation | Comparer les trois écrans de référence en français. Vérifier 320, 390, 768 et 1440 px, ainsi que le zoom à 200 %. Tester une longue playlist, une pochette absente et un thème d’instance. Le lecteur et les onglets laissent la dernière ligne accessible. |
| Clavier et accessibilité | Ajouter le lien d’évitement vers `main-content`. Parcourir les deux cadres au clavier, vérifier noms accessibles, focus visible, ouverture de la recherche avec ⌘K/Ctrl+K, fermeture avec Échap et restitution du focus. Vérifier les curseurs de lecture, l’annonce d’une erreur de formulaire et un parcours au lecteur d’écran. |
| Métal bleu | Avec WebGL, déplacer le pointeur jusque sur la légende puis sortir du panneau. Sans WebGL, vérifier la vidéo locale et son reflet. En mouvement réduit, vérifier que le poster reste fixe. Sur écran tactile, la page et le formulaire restent utilisables. |
| Données et droits | Exécuter les scénarios de chaque lot avec invité, membre, autre propriétaire et admin selon le parcours. Distinguer catalogue vide, indisponibilité d’une rubrique et démo hors ligne. Vérifier les cinq langues pour chaque libellé ajouté. |
| Lecture et performance | Tester les contrôles dans Chromium et Safari sur iPhone, notamment lecture après un geste, reprise et déplacement dans le fichier. Comparer à la référence du lot 1 les requêtes initiales, le poids chargé et le délai jusqu’au premier son sur la même machine et le même fichier. Examiner toute régression avant publication. |

### Rejouer la recette

Depuis la racine, avec Node 24 et Docker Compose disponibles, exécuter les deux commandes suivantes. Le script prépare PostgreSQL avec pgvector, le vrai backend, le bundle SvelteKit et la configuration Nginx du projet. Il crée ses comptes et ses WAV sur un port local dynamique et nettoie son propre projet Compose en sortie.

```sh
(cd frontend && npm ci)
scripts/test-private-listening.sh
```

Pour tester des images déjà disponibles, définir `QA_USE_BUILT_IMAGES=1`, `QA_FRONTEND_IMAGE` et `QA_BACKEND_IMAGE`. Pour répéter un retour arrière sur les mêmes données, fournir également `QA_ROLLBACK_FRONTEND_IMAGE` avec le digest d’une version ayant réussi le test de lecture privée. Le script refait le test de lecture sur cette version, puis sur la candidate restaurée. Les identifiants fictifs sont générés pour chaque exécution. [Script de recette](../scripts/test-private-listening.sh) · [Scénarios Cypress](../frontend/cypress/private) · [CI](../.github/workflows/ci.yml)

### Première recette sur les bundles Node

| Contrôle | Résultat observé |
| --- | --- |
| Compilation et tests | La compilation de production aboutit sous Node 24. Svelte Check signale zéro erreur et zéro avertissement. Les 646 tests Vitest et les 60 tests Rust ciblés sur l’authentification passent. Vitest couvre 91,70 % des lignes et 79,20 % des branches du périmètre mesuré. |
| Navigateur, proxy et données | Huit scénarios Cypress réussis deux fois de suite sur la candidate, avec zéro relance automatique. Deux initialisations indépendantes ont produit les comptes, les deux pistes de 24 s, les playlists et les 101 imports attendus. |
| Lecture | Accès anonyme refusé, lecture native et plages HTTP 206 après connexion, pause, déplacement à 7 s, navigation, piste suivante, perte de l’autorisation média, réessai puis déconnexion. |
| Catalogue et droits | Recherche → artiste → album → favoris, accès à la 21e playlist et au 101e import. Après une réponse perdue, la nouvelle tentative laisse une entrée unique. Le second membre reçoit un refus pour la playlist privée. |
| Interface | Vues contrôlées à 320, 390, 768 et 1440 px. À 320 px, la dernière piste reste accessible. Le zoom natif à 200 %, la navigation clavier, la restitution du focus, l’état d’erreur accessible et le poster fixe en mouvement réduit ont été contrôlés dans Chromium. Le métal réagit aussi au survol de la légende en WebGL et en mode vidéo. |
| Réglages et administration | Le volume persiste après rechargement. Les réglages lecture utilisent les stores du lecteur. Annuler la suppression rend le focus au bouton. L’accès direct à l’administration distingue membre et admin. |
| Retour arrière local | Sur le même backend et les mêmes données, les deux scénarios de lecture passent après bascule Nginx vers le bundle intermédiaire des lots d’écoute, puis vers le bundle des lots suivants. L’ancienne référence reproduisant le 401 est exclue des versions de repli qualifiées. |

**Mesure locale sur l’album.** Avec Chromium headless à 1440 px et le cache désactivé, la référence charge 54 ressources (189 068 octets transférés) puis reçoit un 401 sur le WAV. La candidate en charge 63 (199 785 octets). Après le clic, 430 ms s’écoulent avant que le lecteur dépasse 0,1 s sur le morceau. Il s’agit d’un relevé sur la machine de recette.

**Versions vérifiées.** L’empreinte SHA-256 du contenu du bundle intermédiaire est `0d0c4fae8bbd5bf0e50bf54528340c5d1dbdd42249552b17b918702f8e4efa9e`. Celle du bundle de livraison de la seconde instance est `e4c53e7fad707e8f3b138034f05ea504bd67fe6bdd461ea5e1a63dc902ec1df0`. Pour les reproduire, trier les fichiers de `build/` par chemin relatif et hacher la concaténation du chemin UTF-8, d’un octet nul et du SHA-256 binaire du contenu de chaque fichier.

### Qualification Docker sur instance privée

Le 19 novembre 2026, la recette a vérifié les identifiants des deux images effectivement démarrées derrière Nginx, avec PostgreSQL et les inscriptions fermées. La capture frontend date de 17 h 30 CET. Le serveur reprend la capture de 16 h 46 et les corrections de compilation et de lint associées.

| Image locale Linux ARM64 | Identifiant Docker |
| --- | --- |
| Frontend Explorer | `sha256:1f147bf72e43917f6c53aac51f6cc90494f37e7b3c78f156cd586ca73fbff3d0` |
| Serveur | `sha256:520714e46cb43d766c47633c960c186247cf0ade5ea32d482bcb8f702a6f493a` |
| Frontend de retour arrière | `sha256:914f67c8e836a090ea280c0da71b90fa52c99bd604d0c6f4dd4d76929dbd5cb7` |

Les neuf scénarios Cypress ont réussi. Ils couvrent notamment le rechargement de l’onglet Genres, le retour navigateur, la recherche suivie d’une écoute, les favoris, les playlists et les droits d’administration. Deux parcours d’écoute ont réussi sur l’image de repli, puis sur la candidate restaurée, avec le même backend et les mêmes médias.

WebKit 26.5 en mode headless à 390 × 844 a vérifié la lecture tactile, une plage HTTP 206, la pause, le déplacement à sept secondes et la reprise. Le contrôle visuel d’Explorer a couvert 320, 390 et 1440 px. Sur téléphone, le champ de recherche occupe la largeur disponible et les albums défilent sur une seule rangée au-dessus des premiers titres.

Les 652 tests Vitest passent, ainsi que 277 tests du serveur et 302 tests P2P. Svelte Check, le typage Cypress, rustfmt et Clippy passent. Une base historique a migré du schéma 34 au schéma 42 et le rejeu a été vérifié.

**Avant promotion.** Le contrôle CI sélectionne les deux digests Linux AMD64 produits pour la révision et exécute la recette avant la publication des tags. La recette locale utilise des images Linux ARM64. La CI doit encore qualifier les digests Linux AMD64 destinés au registre. Le contrôle sur iPhone physique et le parcours au lecteur d’écran restent à réaliser.

## Mise en production et retour arrière

Publier les extensions d’API compatibles avant le frontend qui les consomme. Valider la version candidate sur une instance pilote avec le même proxy qu’en production, en rechargeant directement une fiche d’album et la page de connexion. Vérifier les ressources statiques, la lecture privée et la création d’une playlist. Conserver les deux digests frontend, celui du pilote et celui de la version précédente.

Pour revenir en arrière, sélectionner le digest précédent dans la surcharge Compose et recréer uniquement le service `frontend`. Vérifier de nouveau, depuis Nginx, l’accueil, la connexion et la lecture. Tester cette combinaison avec le backend mis à niveau avant la promotion. Le plan de retour préserve les volumes, les comptes et les médias. [Services Compose](../docker-compose.yml) · [Proxy Nginx](../docker/nginx.conf) · [Image frontend](../docker/Dockerfile.frontend)
