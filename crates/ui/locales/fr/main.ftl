app-name = Oxidcord
badge-bot = BOT

rail-home = Messages privés
rail-add-server = Ajouter un serveur
rail-explore = Explorer les serveurs publics

menu-mark-as-read = Marquer comme lu
menu-mute-server = Rendre le serveur muet
menu-mute-channel = Rendre le salon muet
menu-server-settings = Paramètres du serveur
menu-notification-settings = Paramètres de notification
menu-invite-people = Inviter des gens
menu-create-channel = Créer un salon
menu-copy-id = Copier l'identifiant
menu-copy-link = Copier le lien
activity-playing = Joue
activity-streaming = En direct
activity-listening = Écoute { $name }
activity-watching = Regarde { $name }
activity-competing = En compétition sur { $name }
attachment-spoiler = Marquer comme spoiler
attachment-unspoiler = Retirer le spoiler
attachment-modify = Modifier la pièce jointe
attachment-modify-title = Modifier la pièce jointe
attachment-remove = Retirer la pièce jointe
attachment-spoiler-tag = SPOILER
attachment-filename = Nom du fichier
attachment-description = Description (texte alternatif)
attachment-description-placeholder = Ajoute une description
attachment-mark-spoiler = Marquer comme spoiler
attachment-crop-reset = Réinitialiser le recadrage
menu-view-raw = Voir les données brutes
menu-copy-thread-id = Copier l'identifiant du fil
menu-copy-post-id = Copier l'identifiant de la publication
menu-leave-server = Quitter le serveur

tooltip-create-channel = Créer un salon
tooltip-create-dm = Créer un MP

dm-search = Rechercher ou lancer une conversation
dm-friends = Amis
dm-nitro = Nitro
dm-shop = Boutique
dm-direct-messages = Messages privés
dm-group-members = { $count ->
    [one] { $count } membre
   *[other] { $count } membres
}

user-mute = Rendre muet
user-unmute = Réactiver le micro
user-deafen = Mettre en sourdine
user-undeafen = Réactiver le son
user-settings = Paramètres utilisateur

top-bar-threads = Fils
top-bar-notifications = Paramètres de notification
top-bar-pinned = Messages épinglés
top-bar-show-members = Afficher la liste des membres
top-bar-hide-members = Masquer la liste des membres
top-bar-search = Rechercher
top-bar-inbox = Boîte de réception
top-bar-help = Aide
top-bar-call = Démarrer un appel vocal
top-bar-video = Démarrer un appel vidéo

members-online = En ligne — { $count }
members-offline = Hors ligne — { $count }
members-role = { $role } — { $count }

timeline-welcome-title = Bienvenue dans #{ $name } !
timeline-welcome-body = C'est le début du salon #{ $name }.
timeline-welcome-direct-title = { $name }
timeline-welcome-direct-body = C'est le début de ton historique de messages privés avec { $name }.
timeline-new = Nouveau
timeline-jump-latest = Aller aux plus récents

message-react = Ajouter une réaction
message-reply = Répondre
message-more = Plus

composer-placeholder = Envoyer un message dans #{ $name }
composer-placeholder-direct = Envoyer un message à @{ $name }
composer-no-permission = Tu n'as pas la permission d'envoyer des messages dans ce salon.
composer-attach = Envoyer un fichier
composer-gift = Envoyer un cadeau
composer-gif = Ouvrir le sélecteur de GIF
composer-sticker = Ouvrir le sélecteur d'autocollants
composer-emoji = Choisir un émoji
composer-typing-one = { $name } est en train d'écrire...
composer-typing-two = { $first } et { $second } sont en train d'écrire...
composer-typing-several = Plusieurs personnes sont en train d'écrire...

quick-switcher-placeholder = Où veux-tu aller ?
quick-switcher-empty = Aucun résultat pour ta recherche.
quick-switcher-tip = ASTUCE : commence ta recherche par @, #, ! ou * pour affiner les résultats.
quick-switcher-dm = Message privé

settings-section-app = Paramètres de l'application
settings-section-oxidcord = Oxidcord
settings-my-account = Mon compte
settings-profiles = Profils
settings-privacy = Confidentialité
settings-devices = Appareils
settings-connections = Connexions
settings-clips = Clips
settings-friend-requests = Demandes d'ami
settings-appearance = Apparence
settings-accessibility = Accessibilité
settings-voice-video = Voix et vidéo
settings-chat = Discussion
settings-notifications = Notifications
settings-keybinds = Raccourcis clavier
settings-language = Langue
settings-streamer-mode = Mode streamer
settings-advanced = Avancés
settings-plugins = Plugins
settings-themes = Thèmes
settings-developer = Développeur
settings-log-out = Déconnexion
settings-version = Oxidcord { $version }
settings-close-hint = ÉCHAP
settings-theme = Thème
settings-theme-dark = Sombre
settings-theme-light = Clair
settings-theme-amoled = AMOLED
settings-font-scaling = Taille de la police
settings-dpi-scale = Échelle de l'interface
settings-dpi-hint = Ctrl + et Ctrl - zooment l'interface, Ctrl 0 la réinitialise.
settings-message-display = Affichage des messages
settings-display-cozy = Confortable : moderne, joli et facile à lire.
settings-display-compact = Compact : plus de messages à l'écran en même temps.
settings-motion = Animations réduites
settings-reduced-motion = Réduire les animations et les transitions

action-cancel = Annuler

menu-copy-text = Copier le texte
menu-copy-user-id = Copier l'identifiant de l'utilisateur
menu-profile = Profil
menu-message = Envoyer un message
menu-add-friend = Ajouter en ami
menu-pin = Épingler le message
menu-mark-unread = Marquer comme non lu
menu-delete = Supprimer le message

user-latency = Latence : { $ms } ms
user-connecting = Connexion

connection-reconnecting = Reconnexion...
connection-retry = Réessayer

login-title = Content de te revoir
login-subtitle = Colle le jeton de ton compte pour continuer.
login-token-label = Jeton
login-token-placeholder = Jeton du compte
login-paste = Coller le jeton
login-qr = Code QR
login-web = Connexion web
login-coming-soon = Bientôt disponible
login-checking = Vérification du jeton...
login-empty = Saisis ou copie d'abord un jeton.
login-rejected = Ce jeton a été refusé.
login-network = Impossible de joindre Discord.

time-today = Aujourd'hui à { $time }
time-yesterday = Hier à { $time }
time-seconds = { $count ->
    [one] { $count } seconde
   *[other] { $count } secondes
}
time-minutes = { $count ->
    [one] { $count } minute
   *[other] { $count } minutes
}
time-hours = { $count ->
    [one] { $count } heure
   *[other] { $count } heures
}
time-days = { $count ->
    [one] { $count } jour
   *[other] { $count } jours
}
time-ago = il y a { $span }
time-in = dans { $span }

message-edit = Modifier le message
message-edit-hint = Entrée pour enregistrer, Échap pour annuler
message-edited = (modifié)
message-deleted = (supprimé)
message-failed = Le message n'a pas pu être envoyé.
message-retry = Réessayer
message-reply-deleted = Le message d'origine a été supprimé
message-reply-attachment = Clique pour voir la pièce jointe
message-forwarded = Transféré

attachment-download = Télécharger

media-play = Lire
media-pause = Pause
media-replay = Relire
media-mute = Couper le son
media-unmute = Réactiver le son
media-unsupported = Impossible de lire ce fichier
embed-open-browser = Ouvrir dans le navigateur

poll-votes = { $count ->
    [one] { $count } vote
   *[other] { $count } votes
}

system-join = { $name } a rejoint le serveur.
system-pin = { $name } a épinglé un message dans ce salon.
system-thread = { $name } a lancé un fil : { $thread }
system-boost = { $name } vient de booster le serveur !
system-boost-tier = { $name } vient de booster le serveur ! Il a atteint le niveau { $tier }.
system-recipient-add = { $name } a ajouté { $target } au groupe.
system-recipient-remove = { $name } a retiré { $target } du groupe.
system-call = { $name } a lancé un appel.
system-rename = { $name } a renommé le salon : { $channel }
system-icon = { $name } a changé l'icône du groupe.
system-generic = { $name } a envoyé un message système.

delete-title = Supprimer le message
delete-body = Tu es sûr de vouloir supprimer ce message ? Cette action est irréversible.

composer-replying = Réponse à { $name }
composer-cancel-reply = Annuler la réponse
composer-remove-file = Retirer

emoji-search = Trouve l'émoji parfait
emoji-results = Résultats de recherche
emoji-server = Émojis du serveur
emoji-people = Émoticônes et personnes
emoji-nature = Animaux et nature
emoji-food = Nourriture et boissons
emoji-activities = Activités
emoji-travel = Voyages et lieux
emoji-objects = Objets
emoji-symbols = Symboles

profile-roles = Rôles
profile-member-since = Membre depuis
profile-message = Envoyer un message

search-searching = Recherche...
search-results = { $count ->
    [one] { $count } résultat
   *[other] { $count } résultats
}
search-close = Fermer les résultats

lightbox-loading = Chargement de l'image...
lightbox-open = Ouvrir l'original
lightbox-close = Fermer

link-title = Quitter Oxidcord ?
link-body = Ce lien mène vers un site externe :
link-trust = Toujours faire confiance à { $host }
link-open = Ouvrir le lien

voice-live = En direct
voice-connecting = Connexion...
voice-connected = Connecté au vocal
voice-reconnecting = Reconnexion...
voice-not-connected = Non connecté
voice-disconnected = Déconnecté
voice-ping = { $ms } ms
voice-decibels = { $db } dB
voice-disconnect = Se déconnecter
voice-retry = Se reconnecter
voice-camera-on = Activer la caméra
voice-camera-off = Désactiver la caméra
voice-screen-share = Partager ton écran
voice-screen-stop = Arrêter le stream
voice-stats-title = Connexion
voice-stats-ping = Ping
voice-stats-loss = Paquets perdus
voice-stats-jitter = Gigue
voice-stats-unknown = Inconnu
voice-user-volume = Volume de l'utilisateur
voice-user-mute = Rendre muet
voice-watch-stream = Regarder le stream
voice-empty = Personne n'est encore dans ce salon.
voice-join = Rejoindre le vocal
voice-join-stage = Rejoindre la conférence
voice-chat-show = Afficher le chat
voice-chat-hide = Masquer le chat
voice-stage-audience = Public - { $count }
voice-incoming-call = Appel entrant
voice-call-accept = Accepter
voice-call-decline = Refuser
voice-picker-title = Partager ton écran
voice-picker-screens = Écrans
voice-picker-windows = Fenêtres
voice-picker-quality = Qualité du stream
voice-picker-share-audio = Partager le son
voice-picker-audio-unavailable = Le partage du son n'est pas encore pris en charge
voice-picker-go-live = Passer en direct
voice-picker-loading = Recherche des écrans...
voice-picker-empty = Rien à partager ici.
voice-quality-720-30 = 720p 30
voice-quality-1080-30 = 1080p 30
voice-quality-1080-60 = 1080p 60
voice-quality-source-60 = Source 60
voice-ptt = PTT
voice-ptt-indicator = Appuyer-pour-parler est activé
voice-ptt-hint = Configure la touche Appuyer-pour-parler dans Raccourcis clavier.
voice-keybind-push-to-talk = Appuyer pour parler
voice-keybind-toggle-mute = Activer/couper le micro
voice-keybind-toggle-deafen = Activer/couper le son
voice-keybind-press = Appuie sur une touche...
voice-keybind-unset = Non défini
voice-keybind-record = Enregistrer le raccourci
voice-keybind-clear = Effacer le raccourci
voice-keybind-invalid = Cette combinaison de touches ne peut pas être utilisée.
voice-keybind-hint = Les raccourcis fonctionnent même quand Oxidcord est en arrière-plan.
voice-device-default = Par défaut
voice-input-device = Périphérique d'entrée
voice-output-device = Périphérique de sortie
voice-input-volume = Volume d'entrée
voice-output-volume = Volume de sortie
voice-mic-test = Test du micro
voice-mic-check = C'est parti
voice-mic-testing = Test en cours...
voice-mic-test-running = Parle maintenant, tu vas t'entendre pendant { $seconds } secondes.
voice-mic-test-failed = Le test n'a pas pu démarrer. Vérifie tes périphériques.
voice-input-mode = Mode d'entrée
voice-mode-activity = Détection de la voix
voice-mode-ptt = Appuyer pour parler
voice-auto-sensitivity = Déterminer automatiquement la sensibilité d'entrée
voice-sensitivity = Sensibilité d'entrée
voice-noise-suppression = Suppression du bruit
voice-noise-suppression-label = Supprimer le bruit de fond
voice-camera = Caméra
voice-camera-none = Aucune caméra trouvée
voice-camera-preview-off = L'aperçu de la caméra est désactivé
voice-camera-test = Tester la vidéo
voice-camera-test-stop = Arrêter le test
voice-video-quality = Qualité vidéo par défaut

plugin-panel-toggle = Panneau des plugins
plugin-toast-dismiss = Ignorer
plugin-via = via { $name }
plugin-edited-by = Modifié par { $name }
plugin-hidden-by = Masqué par { $name }
plugin-collapsed-by = Réduit par { $name }
plugin-show = Afficher
plugin-hide-again = Masquer
plugin-slot-empty = Cette page de plugin n'est plus disponible.

plugin-tab-installed = Installés
plugin-tab-catalog = Catalogue
plugin-install-file = Installer depuis un fichier
plugin-file-filter = Plugin Oxidcord
plugin-safe-mode = Mode sans échec
plugin-safe-mode-note = Démarre Oxidcord avec tous les plugins désactivés. S'applique après un redémarrage.
plugin-safe-banner = Le mode sans échec est actif. Les plugins restent désactivés jusqu'à ce que tu redémarres sans lui.
plugin-review-banner = Certains plugins demandent des permissions que tu n'as pas encore vérifiées.
plugin-empty = Aucun plugin n'est installé.
plugin-by = par { $author }
plugin-unknown-author = Auteur inconnu
plugin-trust-unsigned = Non signé
plugin-trust-signed = Signé
plugin-trust-catalog = Catalogue
plugin-state-loaded = Chargé
plugin-state-disabled = Désactivé
plugin-state-crashed = Planté
plugin-state-incompatible = Incompatible
plugin-reload = Recharger
plugin-settings = Paramètres
plugin-uninstall = Désinstaller
plugin-review = Vérifier les permissions
plugin-back = Plugins
plugin-uninstall-title = Désinstaller le plugin
plugin-uninstall-body = { $name } et ses données enregistrées seront supprimés. Cette action est irréversible.
plugin-form-empty = Ce plugin n'a aucun paramètre.
plugin-form-add-tag = Saisis une valeur et appuie sur Entrée
plugin-form-range = De { $min } à { $max }

catalog-url = URL du catalogue
catalog-refresh = Actualiser
catalog-install = Installer
catalog-installed = Installé
catalog-reviewed = Vérifié
catalog-empty = Le catalogue est vide. Actualise-le pour le charger.
catalog-permissions = { $count ->
    [one] { $count } permission
   *[other] { $count } permissions
}
catalog-consent-title = Télécharger le catalogue ?
catalog-consent-body = Oxidcord va télécharger { $url } et sa signature via HTTPS.

perm-title = Autoriser { $name } ?
perm-version = Version { $version }
perm-requests = Ce plugin demande à
perm-hosts = Hôtes réseau
perm-none = Ce plugin ne demande aucune permission.
perm-allow-all = Tout autoriser
perm-choose = Choisir
perm-allow-selected = Autoriser la sélection
perm-back = Retour
perm-install = Installer
perm-sensitive = Sensible
perm-context-basic = Lire ton nom d'utilisateur et le salon ouvert.
perm-context-timeline = Lire les messages, les réactions et qui écrit dans les salons des serveurs.
perm-context-directory = Lire la liste de tes serveurs et salons.
perm-context-members = Consulter les noms d'affichage des membres.
perm-context-voice = Lire l'état des salons vocaux.
perm-context-private = Inclure les messages privés dans tout ce qu'il peut lire.
perm-message-render = Changer l'apparence des messages affichés.
perm-message-send = Lire et réécrire ou annuler tes messages avant leur envoi.
perm-message-filter = Masquer des messages de la discussion.
perm-user-display = Changer les noms, couleurs et badges affichés pour les gens.
perm-actions-send = Envoyer des messages en ton nom.
perm-actions-react = Ajouter des réactions en ton nom.
perm-actions-navigate = Changer le salon que tu regardes.
perm-actions-clipboard = Copier du texte dans ton presse-papiers.
perm-actions-notify = Afficher des notifications de bureau.
perm-actions-open-url = Ouvrir des liens dans ton navigateur.
perm-storage = Conserver ses propres données sur cet ordinateur.
perm-net-fetch = Contacter les hôtes réseau listés ci-dessous.
perm-ui-slots = Ajouter des boutons, panneaux et commandes à l'interface.
perm-ui-mount = Ajouter des pages, panneaux, éléments de menu et avis à l'interface.
perm-message-intercept = Changer l'apparence des messages reçus, les masquer ou y ajouter des notes.
perm-message-retain = Garder les messages supprimés visibles dans la discussion.
perm-composer = Insérer du texte dans la zone de message quand tu choisis quelque chose.
perm-context-expressions = Lire les émojis et autocollants que ton compte peut utiliser.
perm-actions-send-user = Envoyer des messages en ton nom sans marque de plugin.
perm-channels-hidden = Afficher dans la liste les salons que tu ne peux pas ouvrir.
perm-high = Risque élevé
perm-image-hosts = Hôtes d'images
perm-low-summary = { $count ->
    [one] 1 permission à faible risque
   *[other] { $count } permissions à faible risque
}

themes-builtin = Intégrés
themes-from-plugins = Depuis les plugins
themes-apply = Appliquer
themes-applied = Appliqué
themes-open-folder = Ouvrir le dossier des thèmes
themes-empty = Aucun plugin de thème installé.
themes-downloaded = Téléchargés
themes-catalog = Catalogue de thèmes

developer-debug = Panneau de débogage des plugins
developer-debug-note = Affiche en bas de la fenêtre les appels, les durées, les appels refusés et les plantages de chaque plugin.
developer-render = Temps de rendu des messages
developer-render-time = 95e centile : { $ms } ms
developer-render-none = Pas encore de mesures.

debug-plugin = Plugin
debug-calls = Appels
debug-last = Dernier ms
debug-denied = Refusés
debug-crashes = Plantages

login-qr-title = Se connecter avec un code QR
login-qr-hint = Scanne-le avec l'appli mobile Discord pour te connecter instantanément.
login-qr-locked = Accepte l'avertissement ci-dessous pour utiliser la connexion par QR.
login-qr-starting = Génération du code...
login-qr-scanned = Regarde ton téléphone !
login-qr-expired = Ce code a expiré.
login-qr-cancelled = La connexion a été annulée.
login-qr-error = Connexion par QR impossible : { $reason }
login-qr-refresh = Actualiser
login-web-waiting = Termine la connexion dans la fenêtre qui s'est ouverte...
login-web-cancel = Annuler
login-web-failed = La connexion web a échoué : { $reason }
login-notice = Oxidcord est un client non officiel, sans lien avec Discord. Utiliser un client tiers avec un compte utilisateur peut enfreindre les Conditions d'utilisation de Discord et entraîner la fermeture du compte. Utilise-le à tes risques et périls.
login-understand = J'ai compris

status-online = En ligne
status-idle = Inactif
status-dnd = Ne pas déranger
status-invisible = Invisible
status-offline = Hors ligne

dm-close = Fermer le MP

friends-title = Amis
friends-tab-online = En ligne
friends-tab-all = Tous
friends-tab-pending = En attente
friends-tab-blocked = Bloqués
friends-tab-add = Ajouter un ami
friends-search = Rechercher
friends-online-count = En ligne — { $count }
friends-all-count = Tous les amis — { $count }
friends-incoming-count = Reçues — { $count }
friends-outgoing-count = Envoyées — { $count }
friends-blocked-count = Bloqués — { $count }
friends-empty-online = Aucun ami n'est en ligne pour le moment.
friends-empty-all = Tu n'as pas encore d'amis.
friends-empty-pending = Aucune demande d'ami en attente.
friends-empty-blocked = Tu n'as bloqué personne.
friends-incoming-request = Demande d'ami reçue
friends-outgoing-request = Demande d'ami envoyée
friends-blocked = Bloqué
friends-message = Envoyer un message
friends-more = Plus
friends-accept = Accepter
friends-ignore = Ignorer
friends-cancel = Annuler
friends-unblock = Débloquer
friends-remove = Retirer l'ami
friends-block = Bloquer
friends-add-title = Ajouter un ami
friends-add-hint = Tu peux ajouter des amis grâce à leur nom d'utilisateur Discord.
friends-add-placeholder = Saisis un nom d'utilisateur
friends-add-send = Envoyer une demande d'ami
friends-add-sent = C'est fait ! Ta demande d'ami à { $name } a été envoyée.
friends-active-title = Actif maintenant
friends-active-empty-title = C'est calme pour l'instant...
friends-active-empty-body = Quand un ami lance une activité, comme jouer à un jeu ou traîner en vocal, on l'affichera ici.

profile-add-friend = Ajouter en ami
profile-request-sent = Demande envoyée
profile-block = Bloquer
profile-copy-id = Copier l'identifiant
profile-about-me = À propos de moi
profile-joined-server = A rejoint le { $date }
profile-view-full = Voir le profil complet
profile-mutual-servers = { $count ->
    [one] { $count } serveur en commun
   *[other] { $count } serveurs en commun
}
profile-mutual-friends = { $count ->
    [one] { $count } ami en commun
   *[other] { $count } amis en commun
}
profile-tab-info = Infos utilisateur
profile-tab-servers = Serveurs en commun
profile-tab-friends = Amis en commun
profile-tab-count = { $name } — { $count }
profile-connections = Connexions
profile-no-mutual-servers = Vous n'avez aucun serveur en commun.
profile-no-mutual-friends = Vous n'avez aucun ami en commun.

action-close = Fermer
status-set-custom = Définir un statut personnalisé
status-edit-profile = Modifier le profil
status-switch-accounts = Changer de compte

custom-status-title = Définir un statut personnalisé
custom-status-label = Quoi de neuf ?
custom-status-placeholder = Les renforts sont arrivés !
custom-status-clear-after = Effacer après
custom-status-never = Ne pas effacer
custom-status-30-minutes = 30 minutes
custom-status-1-hour = 1 heure
custom-status-4-hours = 4 heures
custom-status-today = Aujourd'hui
custom-status-clear = Effacer le statut
custom-status-save = Enregistrer

group-search = Rechercher des amis
group-remaining = { $count ->
    [one] Tu peux encore ajouter { $count } ami.
   *[other] Tu peux encore ajouter { $count } amis.
}
group-no-friends = Aucun ami à afficher.
group-create-title = Créer un groupe privé
group-create = Créer le groupe
group-add-title = Ajouter des amis au groupe
group-add = Ajouter
group-leave-title = Quitter le groupe
group-leave-body = Tu ne recevras plus les messages de ce groupe.
group-leave = Quitter le groupe
dm-close-title = Fermer la conversation
dm-close-body = Cette conversation sera retirée de ta liste. Tu pourras la rouvrir à tout moment.
group-remove-member = Retirer du groupe
group-owner = Propriétaire
group-members-title = Membres — { $count }

settings-note-privacy = Les paramètres de confidentialité et de sécurité se gèrent dans l'application web de Discord.
settings-note-connections = Les connexions se gèrent dans l'application web de Discord.
settings-note-clips = Les clips ne sont pas pris en charge dans Oxidcord.

account-web-note = Ça se modifie dans l'application web de Discord.
account-open-web = Ouvrir Discord
account-edit-profile = Modifier le profil utilisateur
account-display-name = Nom d'affichage
account-username = Nom d'utilisateur
account-email = E-mail
account-security = Mot de passe et authentification
account-password = Mot de passe
account-two-factor = Authentification à deux facteurs
account-sign-out = Se déconnecter

language-choose = Langue
language-en-us = English (US)
language-cs = Čeština
language-de = Deutsch
language-es = Español
language-fr = Français
language-hr = Hrvatski
language-ja = 日本語
language-pt-br = Português (Brasil)
language-ru = Русский
language-zh-cn = 简体中文

notifications-desktop = Alertes
notifications-enable = Notifications de bureau
notifications-enable-note = Afficher une notification quand on te mentionne ou que tu reçois un message privé.
notifications-sounds = Son de notification
notifications-sounds-note = Jouer un son à chaque notification.
notifications-content = Afficher le contenu du message
notifications-content-note = Inclure le début du message dans la notification.
notifications-test = Notification de test
notifications-test-button = Envoyer un test
notifications-test-body = Les notifications de bureau fonctionnent.
notifications-test-sent = Notification envoyée.
notifications-test-failed = Impossible d'afficher la notification.

advanced-startup = Démarrage
advanced-open-on-login = Ouvrir Oxidcord à la connexion
advanced-open-on-login-note = Lancer Oxidcord quand tu te connectes à ton ordinateur.
advanced-startup-failed = Impossible de modifier le paramètre de démarrage.
advanced-minimize-to-tray = Réduire dans la zone de notification
advanced-minimize-to-tray-note = Fermer la fenêtre laisse Oxidcord tourner dans la zone de notification.
advanced-developer = Développeur
advanced-developer-mode = Mode développeur
advanced-developer-mode-note = Afficher « Copier l'identifiant » dans tous les menus.
advanced-data = Données
advanced-data-folder = Dossier des données
advanced-plaintext = Stocker la connexion dans un fichier
advanced-plaintext-note = Enregistre ta connexion et la clé du cache sans chiffrement dans le dossier des données au lieu du gestionnaire d'identifiants du système. Toute personne pouvant lire ce dossier peut utiliser ton compte.
advanced-plaintext-failed = Impossible de déplacer ta connexion.
advanced-open-folder = Ouvrir le dossier

profiles-tab-user = Profil utilisateur
profiles-tab-server = Profils de serveur
profiles-display-name = Nom d'affichage
profiles-pronouns = Pronoms
profiles-pronouns-placeholder = Ajoute tes pronoms
profiles-avatar = Avatar
profiles-banner = Bannière
profiles-change-avatar = Changer d'avatar
profiles-change-banner = Changer de bannière
profiles-image-filter = Images
profiles-image-ready = Prêt à enregistrer
profiles-save = Enregistrer les modifications
profiles-reset = Réinitialiser
profiles-preview = Aperçu
profiles-choose-server = Serveur
profiles-nickname = Pseudo sur le serveur

switcher-category-recent = Récents
switcher-category-channels = Salons textuels
switcher-category-voice = Salons vocaux
switcher-category-dms = Messages privés
switcher-category-friends = Amis
switcher-category-servers = Serveurs
settings-updates = Mises à jour
settings-theme-system = Système
updates-current = Version actuelle
updates-idle = Les mises à jour n'ont pas encore été vérifiées.
updates-checking = Recherche de mises à jour...
updates-up-to-date = Tu es à jour.
updates-could-not-check = Impossible de vérifier les mises à jour.
updates-available = La version { $version } est disponible.
updates-downloading = Téléchargement de { $version }...
updates-ready = La version { $version } est prête à être installée.
updates-installing = Installation...
updates-failed = Échec de la mise à jour : { $error }
updates-check = Rechercher des mises à jour
updates-download = Télécharger
updates-restart = Redémarrer
updates-notes = Notes de version
updates-managed = Cette installation est mise à jour par ton gestionnaire de paquets.
updates-check-on-start = Rechercher des mises à jour au démarrage
updates-badge = Prête
updates-development-branch = Branche de développement
updates-prompt-available = Mise à jour disponible
updates-prompt-downloading = Téléchargement
updates-prompt-ready = Mise à jour prête
advanced-renderer = Moteur de rendu
advanced-renderer-note = Choisi par le moteur graphique au lancement.
voice-mic-stop = Arrêter le test
voice-stats-bitrate = Vidéo
voice-kbps = { $kbps } kb/s
voice-e2ee-on = Chiffré de bout en bout
voice-e2ee-off = Non chiffré
voice-stage-request = Demander la parole
voice-stage-cancel-request = Annuler la demande
voice-stop-watching = Arrêter de regarder
voice-picker-audio-note = Le son du système inclut celui de l'appel, sauf si tu utilises un casque
tray-open = Ouvrir
tray-mute = Rendre muet
tray-deafen = Mettre en sourdine
tray-quit = Quitter
accounts-add = Ajouter un compte
accounts-manage = Gérer les comptes
accounts-done = Terminé
accounts-remove = Retirer
feature-nitro-note = Nitro s'achète sur le site de Discord.
feature-shop-note = La boutique se trouve sur le site de Discord.
feature-open = Ouvrir dans le navigateur
friend-requests-title = Qui peut t'envoyer des demandes d'ami
friend-requests-everyone = Tout le monde
friend-requests-mutual-friends = Amis d'amis
friend-requests-mutual-guilds = Membres des serveurs
streamer-enabled = Mode streamer
streamer-enabled-note = Masque les infos sensibles pendant que tu streames.
streamer-hide-personal = Masquer les infos personnelles
streamer-hide-personal-note = Masque ton e-mail et le tag de ton nom d'utilisateur.
streamer-hide-invites = Masquer les liens d'invitation
streamer-hide-invites-note = Masque les liens d'invitation affichés dans l'appli.
streamer-disable-sounds = Désactiver les sons
streamer-disable-sounds-note = Coupe les sons de notification.
devices-loading = Chargement
devices-empty = Aucun appareil à afficher.
devices-unknown = Appareil inconnu
devices-this = Cet appareil
devices-last-used = Dernière utilisation { $time }
devices-log-out = Déconnecter
devices-log-out-others = Déconnecter tous les autres appareils
accessibility-saturation = Saturation
chat-inline-media = Médias intégrés
chat-inline-media-note = Afficher les images et vidéos dans les messages.
chat-link-previews = Aperçus des liens
chat-link-previews-note = Afficher un aperçu des liens dans les messages.
chat-show-reactions = Réactions
chat-show-reactions-note = Afficher les réactions sous les messages.
chat-time-24h = Format 24 heures
chat-time-24h-note = Afficher l'heure en 14:30 au lieu de 2:30 PM.
chat-emoji-autocomplete = Suggestions d'émojis
chat-emoji-autocomplete-note = Proposer des émojis pendant que tu écris.
language-community-note = Les traductions de la communauté sont les bienvenues.

composer-reply-ping = Mentionner l'auteur
composer-reply-ping-on = OUI
composer-reply-ping-off = NON
composer-send = Envoyer le message
component-select-placeholder = Fais un choix
component-spoiler = Spoiler
message-copy-link = Copier le lien du message
message-copy-id = Copier l'identifiant du message
search-filters = Filtres
search-filter-from = D'un utilisateur précis
search-filter-in = Envoyé dans un salon précis
search-filter-has = Contient un type de contenu précis
search-filter-mentions = Mentionne un utilisateur précis
search-filter-before = Avant une date
search-filter-during = À une date
search-filter-after = Après une date
search-filter-pinned = Épinglé ou non
search-filter-authortype = Envoyé par un type d'auteur
search-filter-more = Plus de filtres
search-filter-more-hint = dates, type d'auteur et plus
search-hint-user = utilisateur
search-hint-channel = salon
search-hint-has = lien, intégration ou fichier
search-hint-date = date
search-hint-pinned = true ou false
search-hint-authortype = user, bot ou webhook
search-history = Historique
search-clear-history = Effacer l'historique
search-today = Aujourd'hui
search-yesterday = Hier
search-no-matches = Aucun résultat
menu-bookmark = Mettre le message de côté
menu-reminder = Créer un rappel
menu-reminder-30m = Dans 30 minutes
menu-reminder-1h = Dans 1 heure
menu-reminder-3h = Dans 3 heures
menu-reminder-tomorrow = Demain matin
menu-speak = Lire le message à voix haute
menu-publish = Publier
title-back = Retour
title-forward = Suivant
menu-search-image = Rechercher l'image
menu-copy-image = Copier l'image
menu-save-image = Enregistrer l'image
menu-copy-image-link = Copier le lien de l'image
menu-open-image-link = Ouvrir le lien de l'image
menu-open-link = Ouvrir le lien
menu-copy = Copier
menu-apps = Applications
