# Interface SoundTime

SoundTime occupe toute la fenêtre du navigateur. Sur grand écran, la navigation se trouve à gauche, le catalogue au centre et le morceau en cours à droite, au-dessus des titres à suivre. Le transport occupe toute la largeur en bas. Il appartient au layout SvelteKit, ce qui permet de parcourir la bibliothèque avec le même élément audio pendant la lecture.

## Choisir le composant

| Besoin | Composant ou module |
| --- | --- |
| Surfaces, couleurs, typographie et rayons | [Tokens de thème](../frontend/src/app.css) et [configuration Tailwind](../frontend/tailwind.config.cjs) |
| Action, lien d’action, champ de saisie, sélecteur d’apparence | [Composants shadcn-svelte et contrôles partagés](../frontend/src/lib/components/ui/) |
| Titre de page, titre de rayon avec lien, en-tête de fiche | [CatalogHeader](../frontend/src/lib/components/catalog/CatalogHeader.svelte), [SectionHeading](../frontend/src/lib/components/catalog/SectionHeading.svelte), [ReleaseHeader](../frontend/src/lib/components/catalog/ReleaseHeader.svelte) |
| Pochette, portrait, carte, rayon horizontal | [CatalogArtwork](../frontend/src/lib/components/catalog/CatalogArtwork.svelte), [CatalogCard](../frontend/src/lib/components/catalog/CatalogCard.svelte), [ArtworkRail](../frontend/src/lib/components/catalog/ArtworkRail.svelte) |
| Liste de morceaux, état vide, erreur, chargement | [TrackList](../frontend/src/lib/components/TrackList.svelte), [CatalogState](../frontend/src/lib/components/catalog/CatalogState.svelte), [CatalogLoading](../frontend/src/lib/components/catalog/CatalogLoading.svelte) |
| Sélection à écouter et accès aux genres | [FeaturedSession](../frontend/src/lib/components/home/FeaturedSession.svelte), [GenreShelf](../frontend/src/lib/components/home/GenreShelf.svelte) |
| Navigation, dock et file | [AppShell](../frontend/src/lib/components/shell/AppShell.svelte), [AudioPlayer](../frontend/src/lib/components/AudioPlayer.svelte), [QueuePanel](../frontend/src/lib/components/player/QueuePanel.svelte) |

## Composer un écran

- Utiliser les couleurs sémantiques. `background` définit le fond uni du site, `sidebar` celui de la navigation et `card` celui du contenu. Les surfaces internes peuvent employer `glass-surface`. Le thème décline ces surfaces en gris bleuté clair ou en graphite. Le menu du compte donne accès aux modes clair, sombre et système.
- Utiliser la police système sans empattement pour les titres comme pour les contrôles. Distinguer les titres de page, de section et de morceau par la taille, la graisse et l’espacement. Les durées utilisent des chiffres tabulaires pour éviter les déplacements pendant la lecture.
- Associer chaque image au bon objet du catalogue. `CatalogArtwork` réserve ses dimensions au chargement et gère les pochettes indisponibles. Sur l’accueil, un artiste peut être identifié par son portrait, par une pochette de sa discographie présentée sous forme de disque ou par ses initiales.
- À partir de 1160 px, afficher le panneau d’écoute à côté du contenu. Sa commande dans le lecteur permet de le refermer ou de le rouvrir. En dessous de ce seuil, la file s’ouvre en dialogue. La navigation latérale passe en icônes de 768 à 1000 px, puis en menu sous 768 px.
- Utiliser les requêtes annulables du [catalogue](../frontend/src/lib/catalog/query.svelte.ts) lors d’un changement de page ou de filtre. Les rayons de l’accueil se révèlent ensemble au premier chargement. Distinguer ensuite un catalogue vide d’une erreur pour laisser réessayer la section concernée.
- Pour les dialogues de formulaire, utiliser les comportements de focus de Bits UI avec `ui-overlay` et `ui-dialog`. Le grand lecteur s’ouvre sur toute la surface du site. Ses commandes, le dock et la file utilisent les mêmes stores de lecture. La touche Échap ramène au contrôle qui l’a ouvert.

## Valider une modification

Dans `frontend/`, exécuter `npm run check` et `npm test`. Pour une modification des commandes ou de la navigation, la [recette privée](../scripts/test-private-listening.sh) exerce le parcours derrière Nginx avec un vrai fichier audio et une base jetable. Vérifier également au clavier l’ouverture et la fermeture des panneaux, ainsi que la continuité de lecture pendant un retour navigateur.
