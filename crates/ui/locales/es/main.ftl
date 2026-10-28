app-name = Oxidcord
badge-bot = BOT

rail-home = Mensajes directos
rail-add-server = Añadir un servidor
rail-explore = Explorar servidores públicos

menu-mark-as-read = Marcar como leído
menu-mute-server = Silenciar servidor
menu-mute-channel = Silenciar canal
menu-server-settings = Ajustes del servidor
menu-notification-settings = Ajustes de notificaciones
menu-invite-people = Invitar gente
menu-create-channel = Crear canal
menu-copy-id = Copiar ID
menu-copy-link = Copiar enlace
activity-playing = Jugando
activity-streaming = En directo
activity-listening = Escuchando { $name }
activity-watching = Viendo { $name }
activity-competing = Compitiendo en { $name }
attachment-spoiler = Marcar como spoiler
attachment-unspoiler = Quitar spoiler
attachment-modify = Editar archivo adjunto
attachment-modify-title = Editar archivo adjunto
attachment-remove = Quitar archivo adjunto
attachment-spoiler-tag = SPOILER
attachment-filename = Nombre del archivo
attachment-description = Descripción (texto alternativo)
attachment-description-placeholder = Añade una descripción
attachment-mark-spoiler = Marcar como spoiler
attachment-crop-reset = Restablecer recorte
menu-view-raw = Ver datos sin procesar
menu-copy-thread-id = Copiar ID del hilo
menu-copy-post-id = Copiar ID de la publicación
menu-leave-server = Abandonar servidor

tooltip-create-channel = Crear canal
tooltip-create-dm = Crear MD

dm-search = Busca o inicia una conversación
dm-friends = Amigos
dm-nitro = Nitro
dm-shop = Tienda
dm-direct-messages = Mensajes directos
dm-group-members = { $count ->
    [one] { $count } miembro
   *[other] { $count } miembros
}

user-mute = Silenciar
user-unmute = Activar micrófono
user-deafen = Ensordecer
user-undeafen = Dejar de ensordecer
user-settings = Ajustes de usuario

top-bar-threads = Hilos
top-bar-notifications = Ajustes de notificaciones
top-bar-pinned = Mensajes fijados
top-bar-show-members = Mostrar lista de miembros
top-bar-hide-members = Ocultar lista de miembros
top-bar-search = Buscar
top-bar-inbox = Bandeja de entrada
top-bar-help = Ayuda
top-bar-call = Iniciar llamada de voz
top-bar-video = Iniciar videollamada

members-online = En línea — { $count }
members-offline = Desconectados — { $count }
members-role = { $role } — { $count }

timeline-welcome-title = ¡Te damos la bienvenida a #{ $name }!
timeline-welcome-body = Este es el comienzo del canal #{ $name }.
timeline-welcome-direct-title = { $name }
timeline-welcome-direct-body = Este es el comienzo de tu historial de mensajes directos con { $name }.
timeline-new = Nuevo
timeline-jump-latest = Ir a lo más reciente

message-react = Añadir reacción
message-reply = Responder
message-more = Más

composer-placeholder = Enviar mensaje a #{ $name }
composer-placeholder-direct = Enviar mensaje a @{ $name }
composer-no-permission = No tienes permiso para enviar mensajes en este canal.
composer-attach = Subir un archivo
composer-gift = Enviar un regalo
composer-gif = Abrir selector de GIF
composer-sticker = Abrir selector de stickers
composer-emoji = Elegir emoji
composer-typing-one = { $name } está escribiendo...
composer-typing-two = { $first } y { $second } están escribiendo...
composer-typing-several = Varias personas están escribiendo...

quick-switcher-placeholder = ¿Adónde quieres ir?
quick-switcher-empty = No hay resultados para tu búsqueda.
quick-switcher-tip = CONSEJO: empieza la búsqueda con @, #, ! o * para acotar los resultados.
quick-switcher-dm = Mensaje directo

settings-section-app = Ajustes de la aplicación
settings-section-oxidcord = Oxidcord
settings-my-account = Mi cuenta
settings-profiles = Perfiles
settings-privacy = Privacidad
settings-devices = Dispositivos
settings-connections = Conexiones
settings-clips = Clips
settings-friend-requests = Amistades
settings-appearance = Apariencia
settings-accessibility = Accesibilidad
settings-voice-video = Voz y vídeo
settings-chat = Chat
settings-notifications = Notificaciones
settings-keybinds = Atajos de teclado
settings-language = Idioma
settings-streamer-mode = Modo streamer
settings-advanced = Avanzado
settings-plugins = Plugins
settings-themes = Temas
settings-developer = Desarrollador
settings-log-out = Cerrar sesión
settings-version = Oxidcord { $version }
settings-close-hint = ESC
settings-theme = Tema
settings-theme-dark = Oscuro
settings-theme-light = Claro
settings-theme-amoled = AMOLED
settings-font-scaling = Tamaño de fuente
settings-dpi-scale = Escala de la interfaz
settings-dpi-hint = Ctrl + y Ctrl - amplían o reducen la interfaz; Ctrl 0 la restablece.
settings-message-display = Visualización de mensajes
settings-display-cozy = Cómodo: moderno, bonito y fácil de leer.
settings-display-compact = Compacto: caben más mensajes en pantalla a la vez.
settings-motion = Movimiento reducido
settings-reduced-motion = Reducir animaciones y transiciones

action-cancel = Cancelar

menu-copy-text = Copiar texto
menu-copy-user-id = Copiar ID de usuario
menu-profile = Perfil
menu-message = Enviar mensaje
menu-add-friend = Añadir amigo
menu-pin = Fijar mensaje
menu-mark-unread = Marcar como no leído
menu-delete = Eliminar mensaje

user-latency = Latencia: { $ms } ms
user-connecting = Conectando

connection-reconnecting = Reconectando...
connection-retry = Reintentar

login-title = ¡Hola de nuevo!
login-subtitle = Pega el token de tu cuenta para continuar.
login-token-label = Token
login-token-placeholder = Token de la cuenta
login-paste = Pegar token
login-qr = Código QR
login-web = Inicio de sesión web
login-coming-soon = Próximamente
login-checking = Comprobando token...
login-empty = Primero introduce o copia un token.
login-rejected = Ese token ha sido rechazado.
login-network = No se pudo conectar con Discord.

time-today = Hoy a las { $time }
time-yesterday = Ayer a las { $time }
time-seconds = { $count ->
    [one] { $count } segundo
   *[other] { $count } segundos
}
time-minutes = { $count ->
    [one] { $count } minuto
   *[other] { $count } minutos
}
time-hours = { $count ->
    [one] { $count } hora
   *[other] { $count } horas
}
time-days = { $count ->
    [one] { $count } día
   *[other] { $count } días
}
time-ago = hace { $span }
time-in = dentro de { $span }

message-edit = Editar mensaje
message-edit-hint = Intro para guardar, Escape para cancelar
message-edited = (editado)
message-deleted = (eliminado)
message-failed = No se pudo enviar el mensaje.
message-retry = Reintentar
message-reply-deleted = El mensaje original se ha eliminado
message-reply-attachment = Haz clic para ver el archivo adjunto
message-forwarded = Reenviado

attachment-download = Descargar

media-play = Reproducir
media-pause = Pausar
media-replay = Volver a reproducir
media-mute = Silenciar
media-unmute = Activar sonido
media-unsupported = No se puede reproducir este archivo
embed-open-browser = Abrir en el navegador

poll-votes = { $count ->
    [one] { $count } voto
   *[other] { $count } votos
}

system-join = { $name } se ha unido al servidor.
system-pin = { $name } ha fijado un mensaje en este canal.
system-thread = { $name } ha iniciado un hilo: { $thread }
system-boost = ¡{ $name } acaba de mejorar el servidor!
system-boost-tier = ¡{ $name } acaba de mejorar el servidor! Ha alcanzado el nivel { $tier }.
system-recipient-add = { $name } ha añadido a { $target } al grupo.
system-recipient-remove = { $name } ha eliminado a { $target } del grupo.
system-call = { $name } ha iniciado una llamada.
system-rename = { $name } ha cambiado el nombre del canal: { $channel }
system-icon = { $name } ha cambiado el icono del grupo.
system-generic = { $name } ha enviado un mensaje del sistema.

delete-title = Eliminar mensaje
delete-body = ¿Seguro que quieres eliminar este mensaje? No se puede deshacer.

composer-replying = Respondiendo a { $name }
composer-cancel-reply = Cancelar respuesta
composer-remove-file = Quitar

emoji-search = Encuentra el emoji perfecto
emoji-results = Resultados de búsqueda
emoji-server = Emojis del servidor
emoji-people = Caras y personas
emoji-nature = Animales y naturaleza
emoji-food = Comida y bebida
emoji-activities = Actividades
emoji-travel = Viajes y lugares
emoji-objects = Objetos
emoji-symbols = Símbolos

profile-roles = Roles
profile-member-since = Miembro desde
profile-message = Enviar mensaje

search-searching = Buscando...
search-results = { $count ->
    [one] { $count } resultado
   *[other] { $count } resultados
}
search-close = Cerrar resultados

lightbox-loading = Cargando imagen...
lightbox-open = Abrir original
lightbox-close = Cerrar

link-title = ¿Salir de Oxidcord?
link-body = Este enlace lleva a un sitio externo:
link-trust = Confiar siempre en { $host }
link-open = Abrir enlace

voice-live = En directo
voice-connecting = Conectando...
voice-connected = Voz conectada
voice-reconnecting = Reconectando...
voice-not-connected = Sin conexión
voice-disconnected = Desconectado
voice-ping = { $ms } ms
voice-decibels = { $db } dB
voice-disconnect = Desconectar
voice-retry = Volver a conectar
voice-camera-on = Activar cámara
voice-camera-off = Desactivar cámara
voice-screen-share = Compartir pantalla
voice-screen-stop = Dejar de transmitir
voice-stats-title = Conexión
voice-stats-ping = Ping
voice-stats-loss = Paquetes perdidos
voice-stats-jitter = Jitter
voice-stats-unknown = Desconocido
voice-user-volume = Volumen del usuario
voice-user-mute = Silenciar
voice-watch-stream = Ver transmisión
voice-empty = Todavía no hay nadie en este canal.
voice-join = Unirse a voz
voice-join-stage = Unirse al escenario
voice-chat-show = Mostrar chat
voice-chat-hide = Ocultar chat
voice-stage-audience = Público - { $count }
voice-incoming-call = Llamada entrante
voice-call-accept = Aceptar
voice-call-decline = Rechazar
voice-picker-title = Compartir pantalla
voice-picker-screens = Pantallas
voice-picker-windows = Ventanas
voice-picker-quality = Calidad de transmisión
voice-picker-share-audio = Compartir audio
voice-picker-audio-unavailable = Todavía no se puede compartir audio
voice-picker-go-live = Transmitir
voice-picker-loading = Buscando pantallas...
voice-picker-empty = No hay nada que compartir.
voice-quality-720-30 = 720p 30
voice-quality-1080-30 = 1080p 30
voice-quality-1080-60 = 1080p 60
voice-quality-source-60 = Original 60
voice-ptt = PTT
voice-ptt-indicator = Pulsar para hablar está activado
voice-ptt-hint = Configura la tecla de pulsar para hablar en Atajos de teclado.
voice-keybind-push-to-talk = Pulsar para hablar
voice-keybind-toggle-mute = Activar/desactivar silencio
voice-keybind-toggle-deafen = Activar/desactivar ensordecer
voice-keybind-press = Pulsa una tecla...
voice-keybind-unset = Sin asignar
voice-keybind-record = Grabar atajo
voice-keybind-clear = Borrar atajo
voice-keybind-invalid = No se puede usar esa combinación de teclas.
voice-keybind-hint = Los atajos funcionan aunque Oxidcord esté en segundo plano.
voice-device-default = Predeterminado
voice-input-device = Dispositivo de entrada
voice-output-device = Dispositivo de salida
voice-input-volume = Volumen de entrada
voice-output-volume = Volumen de salida
voice-mic-test = Prueba de micrófono
voice-mic-check = A ver
voice-mic-testing = Probando...
voice-mic-test-running = Habla ahora, te escucharás durante { $seconds } segundos.
voice-mic-test-failed = No se pudo iniciar la prueba. Revisa tus dispositivos.
voice-input-mode = Modo de entrada
voice-mode-activity = Actividad de voz
voice-mode-ptt = Pulsar para hablar
voice-auto-sensitivity = Determinar automáticamente la sensibilidad de entrada
voice-sensitivity = Sensibilidad de entrada
voice-noise-suppression = Supresión de ruido
voice-noise-suppression-label = Suprimir ruido de fondo
voice-camera = Cámara
voice-camera-none = No se ha encontrado ninguna cámara
voice-camera-preview-off = La vista previa de la cámara está desactivada
voice-camera-test = Probar vídeo
voice-camera-test-stop = Detener prueba
voice-video-quality = Calidad de vídeo predeterminada

plugin-panel-toggle = Panel de plugins
plugin-toast-dismiss = Descartar
plugin-via = mediante { $name }
plugin-edited-by = Editado por { $name }
plugin-hidden-by = Ocultado por { $name }
plugin-collapsed-by = Contraído por { $name }
plugin-show = Mostrar
plugin-hide-again = Ocultar
plugin-slot-empty = Esta página del plugin ya no está disponible.

plugin-tab-installed = Instalados
plugin-tab-catalog = Catálogo
plugin-install-file = Instalar desde archivo
plugin-file-filter = Plugin de Oxidcord
plugin-safe-mode = Modo seguro
plugin-safe-mode-note = Inicia Oxidcord con todos los plugins desactivados. Se aplica tras reiniciar.
plugin-safe-banner = El modo seguro está activado. Los plugins seguirán desactivados hasta que reinicies sin él.
plugin-review-banner = Algunos plugins piden permisos que aún no has revisado.
plugin-empty = No hay plugins instalados.
plugin-by = de { $author }
plugin-unknown-author = Autor desconocido
plugin-trust-unsigned = Sin firmar
plugin-trust-signed = Firmado
plugin-trust-catalog = Catálogo
plugin-state-loaded = Cargado
plugin-state-disabled = Desactivado
plugin-state-crashed = Bloqueado
plugin-state-incompatible = Incompatible
plugin-reload = Recargar
plugin-settings = Ajustes
plugin-uninstall = Desinstalar
plugin-review = Revisar permisos
plugin-back = Plugins
plugin-uninstall-title = Desinstalar plugin
plugin-uninstall-body = Se eliminarán { $name } y sus datos guardados. No se puede deshacer.
plugin-form-empty = Este plugin no tiene ajustes.
plugin-form-add-tag = Escribe un valor y pulsa Intro
plugin-form-range = De { $min } a { $max }

catalog-url = URL del catálogo
catalog-refresh = Actualizar
catalog-install = Instalar
catalog-installed = Instalado
catalog-reviewed = Revisado
catalog-empty = El catálogo está vacío. Actualízalo para cargarlo.
catalog-permissions = { $count ->
    [one] { $count } permiso
   *[other] { $count } permisos
}
catalog-consent-title = ¿Descargar el catálogo?
catalog-consent-body = Oxidcord descargará { $url } y su firma mediante HTTPS.

perm-title = ¿Permitir { $name }?
perm-version = Versión { $version }
perm-requests = Este plugin solicita
perm-hosts = Hosts de red
perm-none = Este plugin no solicita ningún permiso.
perm-allow-all = Permitir todo
perm-choose = Elegir
perm-allow-selected = Permitir selección
perm-back = Atrás
perm-install = Instalar
perm-sensitive = Sensible
perm-context-basic = Leer tu nombre de usuario y el canal que tienes abierto.
perm-context-timeline = Leer mensajes, reacciones y quién escribe en los canales de servidores.
perm-context-directory = Leer la lista de tus servidores y canales.
perm-context-members = Consultar los nombres visibles de los miembros.
perm-context-voice = Leer el estado de los canales de voz.
perm-context-private = Incluir los mensajes directos en todo lo que puede leer.
perm-message-render = Cambiar cómo se muestran los mensajes.
perm-message-send = Leer y reescribir o cancelar tus mensajes antes de enviarlos.
perm-message-filter = Ocultar mensajes del chat.
perm-user-display = Cambiar los nombres, colores e insignias que se muestran de las personas.
perm-actions-send = Enviar mensajes en tu nombre.
perm-actions-react = Añadir reacciones en tu nombre.
perm-actions-navigate = Cambiar el canal que estás viendo.
perm-actions-clipboard = Copiar texto al portapapeles.
perm-actions-notify = Mostrar notificaciones de escritorio.
perm-actions-open-url = Abrir enlaces en tu navegador.
perm-storage = Guardar sus propios datos en este equipo.
perm-net-fetch = Conectarse a los hosts de red indicados abajo.
perm-ui-slots = Añadir botones, paneles y comandos a la interfaz.
perm-ui-mount = Añadir páginas, paneles, elementos de menú y avisos a la interfaz.
perm-message-intercept = Cambiar el aspecto de los mensajes recibidos, ocultarlos o añadirles notas.
perm-message-retain = Mantener visibles en el chat los mensajes eliminados.
perm-composer = Escribir texto en el cuadro de mensaje cuando eliges algo.
perm-context-expressions = Leer los emojis y stickers que puede usar tu cuenta.
perm-actions-send-user = Enviar mensajes en tu nombre sin marca de plugin.
perm-channels-hidden = Mostrar en la lista canales que no puedes abrir.
perm-typing-hidden = Ocultar a los demás que estás escribiendo.
perm-high = Riesgo alto
perm-image-hosts = Hosts de imágenes
perm-low-summary = { $count ->
    [one] 1 permiso de riesgo bajo
   *[other] { $count } permisos de riesgo bajo
}

themes-builtin = Integrados
themes-from-plugins = De plugins
themes-apply = Aplicar
themes-applied = Aplicado
themes-open-folder = Abrir carpeta de temas
themes-empty = No hay plugins de temas instalados.
themes-downloaded = Descargados
themes-catalog = Catálogo de temas

developer-debug = Panel de depuración de plugins
developer-debug-note = Muestra en la parte inferior de la ventana las llamadas, los tiempos, las llamadas denegadas y los fallos de cada plugin.
developer-render = Tiempo de renderizado de mensajes
developer-render-time = Percentil 95: { $ms } ms
developer-render-none = Aún no hay muestras.

debug-plugin = Plugin
debug-calls = Llamadas
debug-last = Último ms
debug-denied = Denegadas
debug-crashes = Fallos

login-qr-title = Iniciar sesión con código QR
login-qr-hint = Escanéalo con la app móvil de Discord para iniciar sesión al instante.
login-qr-locked = Acepta el aviso de abajo para usar el inicio de sesión con QR.
login-qr-starting = Generando código...
login-qr-scanned = ¡Mira tu móvil!
login-qr-expired = Este código ha caducado.
login-qr-cancelled = Se ha cancelado el inicio de sesión.
login-qr-error = No se pudo usar el inicio de sesión con QR: { $reason }
login-qr-refresh = Actualizar
login-web-waiting = Completa el inicio de sesión en la ventana que se ha abierto...
login-web-cancel = Cancelar
login-web-failed = Error en el inicio de sesión web: { $reason }
login-notice = Oxidcord es un cliente no oficial y no está afiliado a Discord. Usar un cliente de terceros con una cuenta de usuario puede infringir las Condiciones del Servicio de Discord y provocar el cierre de la cuenta. Úsalo bajo tu propia responsabilidad.
login-understand = Lo entiendo

status-online = En línea
status-idle = Ausente
status-dnd = No molestar
status-invisible = Invisible
status-offline = Desconectado

dm-close = Cerrar MD

friends-title = Amigos
friends-tab-online = En línea
friends-tab-all = Todos
friends-tab-pending = Pendientes
friends-tab-blocked = Bloqueados
friends-tab-add = Añadir amigo
friends-search = Buscar
friends-online-count = En línea — { $count }
friends-all-count = Todos los amigos — { $count }
friends-incoming-count = Recibidas — { $count }
friends-outgoing-count = Enviadas — { $count }
friends-blocked-count = Bloqueados — { $count }
friends-empty-online = Ningún amigo está en línea ahora mismo.
friends-empty-all = Todavía no tienes amigos.
friends-empty-pending = No hay solicitudes de amistad pendientes.
friends-empty-blocked = No has bloqueado a nadie.
friends-incoming-request = Solicitud de amistad recibida
friends-outgoing-request = Solicitud de amistad enviada
friends-blocked = Bloqueado
friends-message = Enviar mensaje
friends-more = Más
friends-accept = Aceptar
friends-ignore = Ignorar
friends-cancel = Cancelar
friends-unblock = Desbloquear
friends-remove = Eliminar amigo
friends-block = Bloquear
friends-add-title = Añadir amigo
friends-add-hint = Puedes añadir amigos con su nombre de usuario de Discord.
friends-add-placeholder = Introduce un nombre de usuario
friends-add-send = Enviar solicitud de amistad
friends-add-sent = ¡Listo! Se ha enviado tu solicitud de amistad a { $name }.
friends-active-title = Activos ahora
friends-active-empty-title = Por ahora todo está tranquilo...
friends-active-empty-body = Cuando un amigo empiece una actividad, como jugar a un juego o pasar el rato en un canal de voz, la mostraremos aquí.

profile-add-friend = Añadir amigo
profile-request-sent = Solicitud enviada
profile-block = Bloquear
profile-copy-id = Copiar ID
profile-about-me = Sobre mí
profile-joined-server = Se unió el { $date }
profile-view-full = Ver perfil completo
profile-mutual-servers = { $count ->
    [one] { $count } servidor en común
   *[other] { $count } servidores en común
}
profile-mutual-friends = { $count ->
    [one] { $count } amigo en común
   *[other] { $count } amigos en común
}
profile-tab-info = Información del usuario
profile-tab-servers = Servidores en común
profile-tab-friends = Amigos en común
profile-tab-count = { $name } — { $count }
profile-connections = Conexiones
profile-no-mutual-servers = No tenéis servidores en común.
profile-no-mutual-friends = No tenéis amigos en común.

action-close = Cerrar
status-set-custom = Establecer estado personalizado
status-edit-profile = Editar perfil
status-switch-accounts = Cambiar de cuenta

custom-status-title = Establece un estado personalizado
custom-status-label = ¿Qué se cuece?
custom-status-placeholder = ¡Ha llegado la ayuda!
custom-status-clear-after = Borrar después de
custom-status-never = No borrar
custom-status-30-minutes = 30 minutos
custom-status-1-hour = 1 hora
custom-status-4-hours = 4 horas
custom-status-today = Hoy
custom-status-clear = Borrar estado
custom-status-save = Guardar

group-search = Buscar amigos
group-remaining = { $count ->
    [one] Puedes añadir { $count } amigo más.
   *[other] Puedes añadir { $count } amigos más.
}
group-no-friends = No hay amigos que mostrar.
group-create-title = Crear MD grupal
group-create = Crear grupo
group-add-title = Añadir amigos al grupo
group-add = Añadir
group-leave-title = Abandonar grupo
group-leave-body = Dejarás de recibir mensajes de este grupo.
group-leave = Abandonar grupo
dm-close-title = Cerrar conversación
dm-close-body = Esta conversación se quitará de tu lista. Puedes volver a abrirla cuando quieras.
group-remove-member = Quitar del grupo
group-owner = Propietario
group-members-title = Miembros — { $count }

settings-note-privacy = Los ajustes de privacidad y seguridad se gestionan en la aplicación web de Discord.
settings-note-connections = Las conexiones se gestionan en la aplicación web de Discord.
settings-note-clips = Oxidcord no admite clips.

account-web-note = Esto se cambia en la aplicación web de Discord.
account-open-web = Abrir Discord
account-edit-profile = Editar perfil de usuario
account-display-name = Nombre visible
account-username = Nombre de usuario
account-email = Correo electrónico
account-security = Contraseña y autenticación
account-password = Contraseña
account-two-factor = Autenticación en dos pasos
account-sign-out = Cerrar sesión

language-choose = Idioma
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

notifications-desktop = Alertas
notifications-enable = Notificaciones de escritorio
notifications-enable-note = Mostrar una notificación cuando te mencionen o recibas un mensaje directo.
notifications-sounds = Sonido de notificación
notifications-sounds-note = Reproducir un sonido con cada notificación.
notifications-content = Mostrar el contenido del mensaje
notifications-content-note = Incluir el principio del mensaje en la notificación.
notifications-test = Notificación de prueba
notifications-test-button = Enviar prueba
notifications-test-body = Las notificaciones de escritorio funcionan.
notifications-test-sent = Notificación enviada.
notifications-test-failed = No se pudo mostrar la notificación.

advanced-startup = Inicio
advanced-open-on-login = Abrir Oxidcord al iniciar sesión
advanced-open-on-login-note = Inicia Oxidcord cuando entras en tu ordenador.
advanced-startup-failed = No se pudo cambiar el ajuste de inicio.
advanced-minimize-to-tray = Minimizar a la bandeja
advanced-minimize-to-tray-note = Al cerrar la ventana, Oxidcord sigue funcionando en la bandeja.
advanced-developer = Desarrollador
advanced-developer-mode = Modo desarrollador
advanced-developer-mode-note = Mostrar «Copiar ID» en todos los menús.
advanced-data = Datos
advanced-data-folder = Carpeta de datos
advanced-plaintext = Guardar el inicio de sesión en un archivo
advanced-plaintext-note = Guarda tu inicio de sesión y la clave de caché sin cifrar en la carpeta de datos en lugar del almacén de credenciales del sistema. Cualquiera que pueda leer esa carpeta podrá usar tu cuenta.
advanced-plaintext-failed = No se pudo mover tu inicio de sesión.
advanced-open-folder = Abrir carpeta

profiles-tab-user = Perfil de usuario
profiles-tab-server = Perfiles de servidor
profiles-display-name = Nombre visible
profiles-pronouns = Pronombres
profiles-pronouns-placeholder = Añade tus pronombres
profiles-avatar = Avatar
profiles-banner = Banner
profiles-change-avatar = Cambiar avatar
profiles-change-banner = Cambiar banner
profiles-image-filter = Imágenes
profiles-image-ready = Listo para guardar
profiles-save = Guardar cambios
profiles-reset = Restablecer
profiles-preview = Vista previa
profiles-choose-server = Servidor
profiles-nickname = Apodo en el servidor

switcher-category-recent = Recientes
switcher-category-channels = Canales de texto
switcher-category-voice = Canales de voz
switcher-category-dms = Mensajes directos
switcher-category-friends = Amigos
switcher-category-servers = Servidores
settings-updates = Actualizaciones
settings-theme-system = Sistema
updates-current = Versión actual
updates-idle = Todavía no se han buscado actualizaciones.
updates-checking = Buscando actualizaciones...
updates-up-to-date = Tienes la última versión.
updates-could-not-check = No se pudieron buscar actualizaciones.
updates-available = La versión { $version } está disponible.
updates-downloading = Descargando { $version }...
updates-ready = La versión { $version } está lista para instalarse.
updates-installing = Instalando...
updates-failed = Error al actualizar: { $error }
updates-check = Buscar actualizaciones
updates-download = Descargar
updates-restart = Reiniciar
updates-notes = Notas de la versión
updates-managed = Esta instalación se actualiza mediante tu gestor de paquetes.
updates-check-on-start = Buscar actualizaciones al iniciar
updates-badge = Lista
updates-development-branch = Rama de desarrollo
updates-prompt-available = Actualización disponible
updates-prompt-downloading = Descargando
updates-prompt-ready = Actualización lista
advanced-renderer = Renderizador
advanced-renderer-note = Lo elige el motor gráfico al iniciar.
voice-mic-stop = Detener prueba
voice-stats-bitrate = Vídeo
voice-kbps = { $kbps } kbps
voice-e2ee-on = Cifrado de extremo a extremo
voice-e2ee-off = Sin cifrar
voice-stage-request = Pedir la palabra
voice-stage-cancel-request = Cancelar solicitud
voice-stop-watching = Dejar de ver
voice-picker-audio-note = El audio del sistema incluye el sonido de la llamada, salvo que uses auriculares
tray-open = Abrir
tray-mute = Silenciar
tray-deafen = Ensordecer
tray-quit = Salir
accounts-add = Añadir una cuenta
accounts-manage = Gestionar cuentas
accounts-done = Listo
accounts-remove = Quitar
feature-nitro-note = Nitro se compra en la web de Discord.
feature-shop-note = La tienda está en la web de Discord.
feature-open = Abrir en el navegador
friend-requests-title = Quién puede enviarte solicitudes de amistad
friend-requests-everyone = Todo el mundo
friend-requests-mutual-friends = Amigos de amigos
friend-requests-mutual-guilds = Miembros del servidor
streamer-enabled = Modo streamer
streamer-enabled-note = Oculta datos sensibles mientras transmites.
streamer-hide-personal = Ocultar información personal
streamer-hide-personal-note = Oculta tu correo y la etiqueta de tu nombre de usuario.
streamer-hide-invites = Ocultar enlaces de invitación
streamer-hide-invites-note = Oculta los enlaces de invitación que aparecen en la app.
streamer-disable-sounds = Desactivar sonidos
streamer-disable-sounds-note = Silencia los sonidos de notificación.
devices-loading = Cargando
devices-empty = No hay dispositivos que mostrar.
devices-unknown = Dispositivo desconocido
devices-this = Este dispositivo
devices-last-used = Último uso: { $time }
devices-log-out = Cerrar sesión
devices-log-out-others = Cerrar sesión en todos los demás dispositivos
accessibility-saturation = Saturación
chat-inline-media = Multimedia integrada
chat-inline-media-note = Mostrar imágenes y vídeos dentro de los mensajes.
chat-link-previews = Vista previa de enlaces
chat-link-previews-note = Mostrar vistas previas de los enlaces en los mensajes.
chat-show-reactions = Reacciones
chat-show-reactions-note = Mostrar las reacciones debajo de los mensajes.
chat-time-24h = Formato de 24 horas
chat-time-24h-note = Mostrar la hora como 14:30 en lugar de 2:30 PM.
chat-emoji-autocomplete = Sugerencias de emojis
chat-emoji-autocomplete-note = Sugerir emojis mientras escribes.
language-community-note = Las traducciones de la comunidad son bienvenidas.

composer-reply-ping = Mencionar al autor
composer-reply-ping-on = SÍ
composer-reply-ping-off = NO
composer-send = Enviar mensaje
component-select-placeholder = Elige una opción
component-spoiler = Spoiler
message-copy-link = Copiar enlace del mensaje
message-copy-id = Copiar ID del mensaje
search-filters = Filtros
search-filter-from = De un usuario concreto
search-filter-in = Enviado en un canal concreto
search-filter-has = Incluye un tipo de contenido concreto
search-filter-mentions = Menciona a un usuario concreto
search-filter-before = Antes de una fecha
search-filter-during = En una fecha
search-filter-after = Después de una fecha
search-filter-pinned = Fijado o no
search-filter-authortype = Enviado por un tipo de autor
search-filter-more = Más filtros
search-filter-more-hint = fechas, tipo de autor y más
search-hint-user = usuario
search-hint-channel = canal
search-hint-has = enlace, inserción o archivo
search-hint-date = fecha
search-hint-pinned = true o false
search-hint-authortype = user, bot o webhook
search-history = Historial
search-clear-history = Borrar historial
search-today = Hoy
search-yesterday = Ayer
search-no-matches = Sin coincidencias
menu-bookmark = Guardar mensaje
menu-reminder = Crear recordatorio
menu-reminder-30m = Dentro de 30 minutos
menu-reminder-1h = Dentro de 1 hora
menu-reminder-3h = Dentro de 3 horas
menu-reminder-tomorrow = Mañana por la mañana
menu-speak = Leer mensaje en voz alta
menu-publish = Publicar
title-back = Atrás
title-forward = Adelante
menu-search-image = Buscar imagen
menu-copy-image = Copiar imagen
menu-save-image = Guardar imagen
menu-copy-image-link = Copiar enlace de la imagen
menu-open-image-link = Abrir enlace de la imagen
menu-open-link = Abrir enlace
menu-copy = Copiar
menu-apps = Aplicaciones
