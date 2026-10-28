app-name = Oxidcord
badge-bot = BOT

rail-home = Mensagens diretas
rail-add-server = Adicionar um servidor
rail-explore = Explorar servidores públicos

menu-mark-as-read = Marcar como lido
menu-mute-server = Silenciar servidor
menu-mute-channel = Silenciar canal
menu-server-settings = Configurações do servidor
menu-notification-settings = Configurações de notificação
menu-invite-people = Convidar pessoas
menu-create-channel = Criar canal
menu-copy-id = Copiar ID
menu-copy-link = Copiar link
activity-playing = Jogando
activity-streaming = Ao vivo
activity-listening = Ouvindo { $name }
activity-watching = Assistindo { $name }
activity-competing = Competindo em { $name }
attachment-spoiler = Marcar como spoiler
attachment-unspoiler = Remover spoiler
attachment-modify = Editar anexo
attachment-modify-title = Editar anexo
attachment-remove = Remover anexo
attachment-spoiler-tag = SPOILER
attachment-filename = Nome do arquivo
attachment-description = Descrição (texto alternativo)
attachment-description-placeholder = Adicione uma descrição
attachment-mark-spoiler = Marcar como spoiler
attachment-crop-reset = Redefinir corte
menu-view-raw = Ver dados brutos
menu-copy-thread-id = Copiar ID do tópico
menu-copy-post-id = Copiar ID da publicação
menu-leave-server = Sair do servidor

tooltip-create-channel = Criar canal
tooltip-create-dm = Criar MD

dm-search = Encontre ou comece uma conversa
dm-friends = Amigos
dm-nitro = Nitro
dm-shop = Loja
dm-direct-messages = Mensagens diretas
dm-group-members = { $count ->
    [one] { $count } membro
   *[other] { $count } membros
}

user-mute = Silenciar
user-unmute = Ativar microfone
user-deafen = Ensurdecer
user-undeafen = Desensurdecer
user-settings = Configurações de usuário

top-bar-threads = Tópicos
top-bar-notifications = Configurações de notificação
top-bar-pinned = Mensagens fixadas
top-bar-show-members = Mostrar lista de membros
top-bar-hide-members = Ocultar lista de membros
top-bar-search = Buscar
top-bar-inbox = Caixa de entrada
top-bar-help = Ajuda
top-bar-call = Iniciar chamada de voz
top-bar-video = Iniciar chamada de vídeo

members-online = Online — { $count }
members-offline = Offline — { $count }
members-role = { $role } — { $count }

timeline-welcome-title = Boas-vindas a #{ $name }!
timeline-welcome-body = Este é o começo do canal #{ $name }.
timeline-welcome-direct-title = { $name }
timeline-welcome-direct-body = Este é o começo do seu histórico de mensagens diretas com { $name }.
timeline-new = Novo
timeline-jump-latest = Ir para as mais recentes

message-react = Adicionar reação
message-reply = Responder
message-more = Mais

composer-placeholder = Conversar em #{ $name }
composer-placeholder-direct = Conversar com @{ $name }
composer-no-permission = Você não tem permissão para enviar mensagens neste canal.
composer-attach = Enviar um arquivo
composer-gift = Enviar um presente
composer-gif = Abrir seletor de GIF
composer-sticker = Abrir seletor de figurinhas
composer-emoji = Escolher emoji
composer-typing-one = { $name } está digitando...
composer-typing-two = { $first } e { $second } estão digitando...
composer-typing-several = Várias pessoas estão digitando...

quick-switcher-placeholder = Para onde você quer ir?
quick-switcher-empty = Nenhum resultado para sua busca.
quick-switcher-tip = DICA: comece a busca com @, #, ! ou * para filtrar os resultados.
quick-switcher-dm = Mensagem direta

settings-section-app = Configurações do app
settings-section-oxidcord = Oxidcord
settings-my-account = Minha conta
settings-profiles = Perfis
settings-privacy = Privacidade
settings-devices = Dispositivos
settings-connections = Conexões
settings-clips = Clipes
settings-friend-requests = Pedidos de amizade
settings-appearance = Aparência
settings-accessibility = Acessibilidade
settings-voice-video = Voz e vídeo
settings-chat = Bate-papo
settings-notifications = Notificações
settings-keybinds = Atalhos de teclado
settings-language = Idioma
settings-streamer-mode = Modo streamer
settings-advanced = Avançado
settings-plugins = Plugins
settings-themes = Temas
settings-developer = Desenvolvedor
settings-log-out = Sair
settings-version = Oxidcord { $version }
settings-close-hint = ESC
settings-theme = Tema
settings-theme-dark = Escuro
settings-theme-light = Claro
settings-theme-amoled = AMOLED
settings-font-scaling = Tamanho da fonte
settings-dpi-scale = Escala da interface
settings-dpi-hint = Ctrl + e Ctrl - aumentam ou diminuem a interface, Ctrl 0 volta ao padrão.
settings-message-display = Exibição das mensagens
settings-display-cozy = Confortável: moderno, bonito e fácil de ler.
settings-display-compact = Compacto: mais mensagens na tela ao mesmo tempo.
settings-motion = Movimento reduzido
settings-reduced-motion = Reduzir animações e transições

action-cancel = Cancelar

menu-copy-text = Copiar texto
menu-copy-user-id = Copiar ID do usuário
menu-profile = Perfil
menu-message = Mensagem
menu-add-friend = Adicionar amigo
menu-pin = Fixar mensagem
menu-mark-unread = Marcar como não lida
menu-delete = Excluir mensagem

user-latency = Latência: { $ms } ms
user-connecting = Conectando

connection-reconnecting = Reconectando...
connection-retry = Tentar de novo

login-title = Que bom te ver de novo
login-subtitle = Cole o token da sua conta para continuar.
login-token-label = Token
login-token-placeholder = Token da conta
login-paste = Colar token
login-qr = Código QR
login-web = Login pela web
login-coming-soon = Em breve
login-checking = Verificando token...
login-empty = Primeiro digite ou copie um token.
login-rejected = Esse token foi recusado.
login-network = Não foi possível conectar ao Discord.

time-today = Hoje às { $time }
time-yesterday = Ontem às { $time }
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
    [one] { $count } dia
   *[other] { $count } dias
}
time-ago = há { $span }
time-in = em { $span }

message-edit = Editar mensagem
message-edit-hint = Enter para salvar, Esc para cancelar
message-edited = (editada)
message-deleted = (excluída)
message-failed = Não foi possível enviar a mensagem.
message-retry = Tentar de novo
message-reply-deleted = A mensagem original foi excluída
message-reply-attachment = Clique para ver o anexo
message-forwarded = Encaminhada

attachment-download = Baixar

media-play = Reproduzir
media-pause = Pausar
media-replay = Reproduzir de novo
media-mute = Silenciar
media-unmute = Ativar som
media-unsupported = Não é possível reproduzir este arquivo
embed-open-browser = Abrir no navegador

poll-votes = { $count ->
    [one] { $count } voto
   *[other] { $count } votos
}

system-join = { $name } entrou no servidor.
system-pin = { $name } fixou uma mensagem neste canal.
system-thread = { $name } começou um tópico: { $thread }
system-boost = { $name } acabou de impulsionar o servidor!
system-boost-tier = { $name } acabou de impulsionar o servidor! Ele chegou ao nível { $tier }.
system-recipient-add = { $name } adicionou { $target } ao grupo.
system-recipient-remove = { $name } removeu { $target } do grupo.
system-call = { $name } começou uma chamada.
system-rename = { $name } mudou o nome do canal: { $channel }
system-icon = { $name } mudou o ícone do grupo.
system-generic = { $name } enviou uma mensagem do sistema.

delete-title = Excluir mensagem
delete-body = Tem certeza de que quer excluir esta mensagem? Isso não pode ser desfeito.

composer-replying = Respondendo a { $name }
composer-cancel-reply = Cancelar resposta
composer-remove-file = Remover

emoji-search = Encontre o emoji perfeito
emoji-results = Resultados da busca
emoji-server = Emojis do servidor
emoji-people = Carinhas e pessoas
emoji-nature = Animais e natureza
emoji-food = Comidas e bebidas
emoji-activities = Atividades
emoji-travel = Viagens e lugares
emoji-objects = Objetos
emoji-symbols = Símbolos

profile-roles = Cargos
profile-member-since = Membro desde
profile-message = Mensagem

search-searching = Buscando...
search-results = { $count ->
    [one] { $count } resultado
   *[other] { $count } resultados
}
search-close = Fechar resultados

lightbox-loading = Carregando imagem...
lightbox-open = Abrir original
lightbox-close = Fechar

link-title = Sair do Oxidcord?
link-body = Este link leva a um site externo:
link-trust = Sempre confiar em { $host }
link-open = Abrir link

voice-live = Ao vivo
voice-connecting = Conectando...
voice-connected = Voz conectada
voice-reconnecting = Reconectando...
voice-not-connected = Desconectado
voice-disconnected = Desconectado
voice-ping = { $ms } ms
voice-decibels = { $db } dB
voice-disconnect = Desconectar
voice-retry = Reconectar
voice-camera-on = Ligar câmera
voice-camera-off = Desligar câmera
voice-screen-share = Compartilhar tela
voice-screen-stop = Parar transmissão
voice-stats-title = Conexão
voice-stats-ping = Ping
voice-stats-loss = Pacotes perdidos
voice-stats-jitter = Jitter
voice-stats-unknown = Desconhecido
voice-user-volume = Volume do usuário
voice-user-mute = Silenciar
voice-watch-stream = Assistir transmissão
voice-empty = Ainda não tem ninguém neste canal.
voice-join = Entrar na voz
voice-join-stage = Entrar no palco
voice-chat-show = Mostrar bate-papo
voice-chat-hide = Ocultar bate-papo
voice-stage-audience = Plateia - { $count }
voice-incoming-call = Chamada recebida
voice-call-accept = Aceitar
voice-call-decline = Recusar
voice-picker-title = Compartilhar tela
voice-picker-screens = Telas
voice-picker-windows = Janelas
voice-picker-quality = Qualidade da transmissão
voice-picker-share-audio = Compartilhar áudio
voice-picker-audio-unavailable = Compartilhar áudio ainda não é possível
voice-picker-go-live = Transmitir
voice-picker-loading = Procurando telas...
voice-picker-empty = Nada para compartilhar aqui.
voice-quality-720-30 = 720p 30
voice-quality-1080-30 = 1080p 30
voice-quality-1080-60 = 1080p 60
voice-quality-source-60 = Original 60
voice-ptt = PTT
voice-ptt-indicator = Aperte para falar está ativado
voice-ptt-hint = Defina a tecla de aperte para falar em Atalhos de teclado.
voice-keybind-push-to-talk = Aperte para falar
voice-keybind-toggle-mute = Alternar silenciar
voice-keybind-toggle-deafen = Alternar ensurdecer
voice-keybind-press = Aperte uma tecla...
voice-keybind-unset = Não definido
voice-keybind-record = Gravar atalho
voice-keybind-clear = Limpar atalho
voice-keybind-invalid = Essa combinação de teclas não pode ser usada.
voice-keybind-hint = Os atalhos funcionam com o Oxidcord em segundo plano.
voice-device-default = Padrão
voice-input-device = Dispositivo de entrada
voice-output-device = Dispositivo de saída
voice-input-volume = Volume de entrada
voice-output-volume = Volume de saída
voice-mic-test = Teste de microfone
voice-mic-check = Vamos testar
voice-mic-testing = Testando...
voice-mic-test-running = Fale agora, você vai se ouvir por { $seconds } segundos.
voice-mic-test-failed = Não foi possível iniciar o teste. Confira seus dispositivos.
voice-input-mode = Modo de entrada
voice-mode-activity = Detecção de voz
voice-mode-ptt = Aperte para falar
voice-auto-sensitivity = Definir a sensibilidade de entrada automaticamente
voice-sensitivity = Sensibilidade de entrada
voice-noise-suppression = Supressão de ruído
voice-noise-suppression-label = Suprimir ruído de fundo
voice-camera = Câmera
voice-camera-none = Nenhuma câmera encontrada
voice-camera-preview-off = A prévia da câmera está desligada
voice-camera-test = Testar vídeo
voice-camera-test-stop = Parar teste
voice-video-quality = Qualidade de vídeo padrão

plugin-panel-toggle = Painel de plugins
plugin-toast-dismiss = Dispensar
plugin-via = via { $name }
plugin-edited-by = Editado por { $name }
plugin-hidden-by = Ocultado por { $name }
plugin-collapsed-by = Recolhido por { $name }
plugin-show = Mostrar
plugin-hide-again = Ocultar
plugin-slot-empty = Esta página do plugin não está mais disponível.

plugin-tab-installed = Instalados
plugin-tab-catalog = Catálogo
plugin-install-file = Instalar de um arquivo
plugin-file-filter = Plugin do Oxidcord
plugin-safe-mode = Modo seguro
plugin-safe-mode-note = Inicia o Oxidcord com todos os plugins desativados. Vale depois de reiniciar.
plugin-safe-banner = O modo seguro está ativado. Os plugins ficam desativados até você reiniciar sem ele.
plugin-review-banner = Alguns plugins pedem permissões que você ainda não revisou.
plugin-empty = Nenhum plugin instalado.
plugin-by = por { $author }
plugin-unknown-author = Autor desconhecido
plugin-trust-unsigned = Sem assinatura
plugin-trust-signed = Assinado
plugin-trust-catalog = Catálogo
plugin-state-loaded = Carregado
plugin-state-disabled = Desativado
plugin-state-crashed = Travou
plugin-state-incompatible = Incompatível
plugin-reload = Recarregar
plugin-settings = Configurações
plugin-uninstall = Desinstalar
plugin-review = Revisar permissões
plugin-back = Plugins
plugin-uninstall-title = Desinstalar plugin
plugin-uninstall-body = { $name } e os dados salvos dele serão removidos. Isso não pode ser desfeito.
plugin-form-empty = Este plugin não tem configurações.
plugin-form-add-tag = Digite um valor e aperte Enter
plugin-form-range = De { $min } a { $max }

catalog-url = URL do catálogo
catalog-refresh = Atualizar
catalog-install = Instalar
catalog-installed = Instalado
catalog-reviewed = Revisado
catalog-empty = O catálogo está vazio. Atualize para carregá-lo.
catalog-permissions = { $count ->
    [one] { $count } permissão
   *[other] { $count } permissões
}
catalog-consent-title = Baixar o catálogo?
catalog-consent-body = O Oxidcord vai baixar { $url } e a assinatura dele via HTTPS.

perm-title = Permitir { $name }?
perm-version = Versão { $version }
perm-requests = Este plugin pede para
perm-hosts = Hosts de rede
perm-none = Este plugin não pede nenhuma permissão.
perm-allow-all = Permitir tudo
perm-choose = Escolher
perm-allow-selected = Permitir selecionadas
perm-back = Voltar
perm-install = Instalar
perm-sensitive = Sensível
perm-context-basic = Ler seu nome de usuário e o canal aberto.
perm-context-timeline = Ler mensagens, reações e quem está digitando nos canais de servidores.
perm-context-directory = Ler a lista dos seus servidores e canais.
perm-context-members = Consultar os nomes exibidos dos membros.
perm-context-voice = Ler o estado dos canais de voz.
perm-context-private = Incluir mensagens diretas em tudo o que ele pode ler.
perm-message-render = Mudar a aparência das mensagens exibidas.
perm-message-send = Ler e reescrever ou cancelar suas mensagens antes de serem enviadas.
perm-message-filter = Ocultar mensagens do bate-papo.
perm-user-display = Mudar nomes, cores e insígnias exibidos para as pessoas.
perm-actions-send = Enviar mensagens como você.
perm-actions-react = Adicionar reações como você.
perm-actions-navigate = Trocar o canal que você está vendo.
perm-actions-clipboard = Copiar texto para a área de transferência.
perm-actions-notify = Mostrar notificações na área de trabalho.
perm-actions-open-url = Abrir links no navegador.
perm-storage = Guardar os próprios dados neste computador.
perm-net-fetch = Acessar os hosts de rede listados abaixo.
perm-ui-slots = Adicionar botões, painéis e comandos à interface.
perm-ui-mount = Adicionar páginas, painéis, itens de menu e avisos à interface.
perm-message-intercept = Mudar a aparência das mensagens recebidas, ocultá-las ou adicionar notas a elas.
perm-message-retain = Manter mensagens excluídas visíveis no bate-papo.
perm-composer = Colocar texto na caixa de mensagem quando você escolhe algo.
perm-context-expressions = Ler os emojis e figurinhas que sua conta pode usar.
perm-actions-send-user = Enviar mensagens como você sem marca de plugin.
perm-channels-hidden = Mostrar na lista canais que você não pode abrir.
perm-typing-hidden = Esconder dos outros que você está digitando.
perm-high = Risco alto
perm-image-hosts = Hosts de imagem
perm-low-summary = { $count ->
    [one] 1 permissão de baixo risco
   *[other] { $count } permissões de baixo risco
}

themes-builtin = Integrados
themes-from-plugins = De plugins
themes-apply = Aplicar
themes-applied = Aplicado
themes-open-folder = Abrir pasta de temas
themes-empty = Nenhum plugin de tema instalado.
themes-downloaded = Baixados
themes-catalog = Catálogo de temas

developer-debug = Painel de depuração de plugins
developer-debug-note = Mostra na parte de baixo da janela as chamadas, os tempos, as chamadas negadas e os travamentos de cada plugin.
developer-render = Tempo de renderização das mensagens
developer-render-time = Percentil 95: { $ms } ms
developer-render-none = Ainda sem amostras.

debug-plugin = Plugin
debug-calls = Chamadas
debug-last = Último ms
debug-denied = Negadas
debug-crashes = Travamentos

login-qr-title = Entrar com código QR
login-qr-hint = Escaneie com o app do Discord no celular para entrar na hora.
login-qr-locked = Aceite o aviso abaixo para usar o login por QR.
login-qr-starting = Gerando código...
login-qr-scanned = Olha o celular!
login-qr-expired = Este código expirou.
login-qr-cancelled = O login foi cancelado.
login-qr-error = Não foi possível usar o login por QR: { $reason }
login-qr-refresh = Atualizar
login-web-waiting = Termine o login na janela que abriu...
login-web-cancel = Cancelar
login-web-failed = O login pela web falhou: { $reason }
login-notice = O Oxidcord é um cliente não oficial e não tem vínculo com o Discord. Usar um cliente de terceiros com uma conta de usuário pode violar os Termos de Serviço do Discord e resultar no encerramento da conta. Use por sua conta e risco.
login-understand = Entendi

status-online = Online
status-idle = Ausente
status-dnd = Não perturbe
status-invisible = Invisível
status-offline = Offline

dm-close = Fechar MD

friends-title = Amigos
friends-tab-online = Online
friends-tab-all = Todos
friends-tab-pending = Pendentes
friends-tab-blocked = Bloqueados
friends-tab-add = Adicionar amigo
friends-search = Buscar
friends-online-count = Online — { $count }
friends-all-count = Todos os amigos — { $count }
friends-incoming-count = Recebidos — { $count }
friends-outgoing-count = Enviados — { $count }
friends-blocked-count = Bloqueados — { $count }
friends-empty-online = Nenhum amigo online agora.
friends-empty-all = Você ainda não tem amigos.
friends-empty-pending = Não há pedidos de amizade pendentes.
friends-empty-blocked = Você não bloqueou ninguém.
friends-incoming-request = Pedido de amizade recebido
friends-outgoing-request = Pedido de amizade enviado
friends-blocked = Bloqueado
friends-message = Mensagem
friends-more = Mais
friends-accept = Aceitar
friends-ignore = Ignorar
friends-cancel = Cancelar
friends-unblock = Desbloquear
friends-remove = Remover amigo
friends-block = Bloquear
friends-add-title = Adicionar amigo
friends-add-hint = Você pode adicionar amigos pelo nome de usuário do Discord.
friends-add-placeholder = Digite um nome de usuário
friends-add-send = Enviar pedido de amizade
friends-add-sent = Pronto! Seu pedido de amizade para { $name } foi enviado.
friends-active-title = Ativos agora
friends-active-empty-title = Por enquanto está tudo calmo...
friends-active-empty-body = Quando um amigo começar uma atividade, como jogar ou ficar num canal de voz, ela vai aparecer aqui.

profile-add-friend = Adicionar amigo
profile-request-sent = Pedido enviado
profile-block = Bloquear
profile-copy-id = Copiar ID
profile-about-me = Sobre mim
profile-joined-server = Entrou em { $date }
profile-view-full = Ver perfil completo
profile-mutual-servers = { $count ->
    [one] { $count } servidor em comum
   *[other] { $count } servidores em comum
}
profile-mutual-friends = { $count ->
    [one] { $count } amigo em comum
   *[other] { $count } amigos em comum
}
profile-tab-info = Informações do usuário
profile-tab-servers = Servidores em comum
profile-tab-friends = Amigos em comum
profile-tab-count = { $name } — { $count }
profile-connections = Conexões
profile-no-mutual-servers = Vocês não têm servidores em comum.
profile-no-mutual-friends = Vocês não têm amigos em comum.

action-close = Fechar
status-set-custom = Definir status personalizado
status-edit-profile = Editar perfil
status-switch-accounts = Trocar de conta

custom-status-title = Defina um status personalizado
custom-status-label = E aí, quais são as novidades?
custom-status-placeholder = O suporte chegou!
custom-status-clear-after = Limpar depois de
custom-status-never = Não limpar
custom-status-30-minutes = 30 minutos
custom-status-1-hour = 1 hora
custom-status-4-hours = 4 horas
custom-status-today = Hoje
custom-status-clear = Limpar status
custom-status-save = Salvar

group-search = Buscar amigos
group-remaining = { $count ->
    [one] Você pode adicionar mais { $count } amigo.
   *[other] Você pode adicionar mais { $count } amigos.
}
group-no-friends = Nenhum amigo para mostrar.
group-create-title = Criar MD em grupo
group-create = Criar grupo
group-add-title = Adicionar amigos ao grupo
group-add = Adicionar
group-leave-title = Sair do grupo
group-leave-body = Você não vai mais receber mensagens deste grupo.
group-leave = Sair do grupo
dm-close-title = Fechar conversa
dm-close-body = Esta conversa será removida da sua lista. Você pode reabri-la quando quiser.
group-remove-member = Remover do grupo
group-owner = Dono
group-members-title = Membros — { $count }

settings-note-privacy = As configurações de privacidade e segurança são gerenciadas no app web do Discord.
settings-note-connections = As conexões são gerenciadas no app web do Discord.
settings-note-clips = O Oxidcord não tem suporte a clipes.

account-web-note = Isso se muda no app web do Discord.
account-open-web = Abrir Discord
account-edit-profile = Editar perfil de usuário
account-display-name = Nome exibido
account-username = Nome de usuário
account-email = E-mail
account-security = Senha e autenticação
account-password = Senha
account-two-factor = Autenticação em dois fatores
account-sign-out = Sair

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
notifications-enable = Notificações na área de trabalho
notifications-enable-note = Mostrar uma notificação quando você for mencionado ou receber uma mensagem direta.
notifications-sounds = Som de notificação
notifications-sounds-note = Tocar um som a cada notificação.
notifications-content = Mostrar conteúdo da mensagem
notifications-content-note = Incluir o começo da mensagem na notificação.
notifications-test = Notificação de teste
notifications-test-button = Enviar teste
notifications-test-body = As notificações na área de trabalho estão funcionando.
notifications-test-sent = Notificação enviada.
notifications-test-failed = Não foi possível mostrar a notificação.

advanced-startup = Inicialização
advanced-open-on-login = Abrir o Oxidcord ao entrar
advanced-open-on-login-note = Inicia o Oxidcord quando você entra no computador.
advanced-startup-failed = Não foi possível mudar a configuração de inicialização.
advanced-minimize-to-tray = Minimizar para a bandeja
advanced-minimize-to-tray-note = Fechar a janela mantém o Oxidcord rodando na bandeja.
advanced-developer = Desenvolvedor
advanced-developer-mode = Modo desenvolvedor
advanced-developer-mode-note = Mostrar "Copiar ID" em todos os menus.
advanced-data = Dados
advanced-data-folder = Pasta de dados
advanced-plaintext = Guardar o login em um arquivo
advanced-plaintext-note = Salva seu login e a chave do cache sem criptografia na pasta de dados, em vez do armazenamento de credenciais do sistema. Qualquer pessoa que consiga ler essa pasta pode usar sua conta.
advanced-plaintext-failed = Não foi possível mover seu login.
advanced-open-folder = Abrir pasta

profiles-tab-user = Perfil de usuário
profiles-tab-server = Perfis de servidor
profiles-display-name = Nome exibido
profiles-pronouns = Pronomes
profiles-pronouns-placeholder = Adicione seus pronomes
profiles-avatar = Avatar
profiles-banner = Banner
profiles-change-avatar = Mudar avatar
profiles-change-banner = Mudar banner
profiles-image-filter = Imagens
profiles-image-ready = Pronto para salvar
profiles-save = Salvar alterações
profiles-reset = Redefinir
profiles-preview = Prévia
profiles-choose-server = Servidor
profiles-nickname = Apelido no servidor

switcher-category-recent = Recentes
switcher-category-channels = Canais de texto
switcher-category-voice = Canais de voz
switcher-category-dms = Mensagens diretas
switcher-category-friends = Amigos
switcher-category-servers = Servidores
settings-updates = Atualizações
settings-theme-system = Sistema
updates-current = Versão atual
updates-idle = Ainda não procuramos atualizações.
updates-checking = Procurando atualizações...
updates-up-to-date = Você está com a versão mais recente.
updates-could-not-check = Não foi possível procurar atualizações.
updates-available = A versão { $version } está disponível.
updates-downloading = Baixando { $version }...
updates-ready = A versão { $version } está pronta para instalar.
updates-installing = Instalando...
updates-failed = A atualização falhou: { $error }
updates-check = Procurar atualizações
updates-download = Baixar
updates-restart = Reiniciar
updates-notes = Notas da versão
updates-managed = Esta instalação é atualizada pelo seu gerenciador de pacotes.
updates-check-on-start = Procurar atualizações ao iniciar
updates-badge = Pronta
updates-development-branch = Ramo de desenvolvimento
updates-prompt-available = Atualização disponível
updates-prompt-downloading = Baixando
updates-prompt-ready = Atualização pronta
advanced-renderer = Renderizador
advanced-renderer-note = Escolhido pelo backend gráfico ao iniciar.
voice-mic-stop = Parar teste
voice-stats-bitrate = Vídeo
voice-kbps = { $kbps } kbps
voice-e2ee-on = Criptografia de ponta a ponta
voice-e2ee-off = Sem criptografia
voice-stage-request = Pedir para falar
voice-stage-cancel-request = Cancelar pedido
voice-stop-watching = Parar de assistir
voice-picker-audio-note = O áudio do sistema inclui o som da chamada, a menos que você use fones de ouvido
tray-open = Abrir
tray-mute = Silenciar
tray-deafen = Ensurdecer
tray-quit = Sair
accounts-add = Adicionar uma conta
accounts-manage = Gerenciar contas
accounts-done = Concluído
accounts-remove = Remover
feature-nitro-note = O Nitro é comprado no site do Discord.
feature-shop-note = A loja fica no site do Discord.
feature-open = Abrir no navegador
friend-requests-title = Quem pode te enviar pedidos de amizade
friend-requests-everyone = Todo mundo
friend-requests-mutual-friends = Amigos de amigos
friend-requests-mutual-guilds = Membros do servidor
streamer-enabled = Modo streamer
streamer-enabled-note = Oculta dados sensíveis enquanto você transmite.
streamer-hide-personal = Ocultar informações pessoais
streamer-hide-personal-note = Oculta seu e-mail e a tag do seu nome de usuário.
streamer-hide-invites = Ocultar links de convite
streamer-hide-invites-note = Oculta os links de convite mostrados no app.
streamer-disable-sounds = Desativar sons
streamer-disable-sounds-note = Silencia os sons de notificação.
devices-loading = Carregando
devices-empty = Nenhum dispositivo para mostrar.
devices-unknown = Dispositivo desconhecido
devices-this = Este dispositivo
devices-last-used = Último uso { $time }
devices-log-out = Sair
devices-log-out-others = Sair de todos os outros dispositivos
accessibility-saturation = Saturação
chat-inline-media = Mídia nas mensagens
chat-inline-media-note = Mostrar imagens e vídeos dentro das mensagens.
chat-link-previews = Prévias de links
chat-link-previews-note = Mostrar prévias dos links nas mensagens.
chat-show-reactions = Reações
chat-show-reactions-note = Mostrar reações embaixo das mensagens.
chat-time-24h = Formato de 24 horas
chat-time-24h-note = Mostrar horários como 14:30 em vez de 2:30 PM.
chat-emoji-autocomplete = Sugestões de emoji
chat-emoji-autocomplete-note = Sugerir emojis enquanto você digita.
language-community-note = Traduções da comunidade são bem-vindas.

composer-reply-ping = Mencionar autor
composer-reply-ping-on = SIM
composer-reply-ping-off = NÃO
composer-send = Enviar mensagem
component-select-placeholder = Faça uma escolha
component-spoiler = Spoiler
message-copy-link = Copiar link da mensagem
message-copy-id = Copiar ID da mensagem
search-filters = Filtros
search-filter-from = De um usuário específico
search-filter-in = Enviada em um canal específico
search-filter-has = Contém um tipo específico de conteúdo
search-filter-mentions = Menciona um usuário específico
search-filter-before = Antes de uma data
search-filter-during = Em uma data
search-filter-after = Depois de uma data
search-filter-pinned = Fixada ou não
search-filter-authortype = Enviada por um tipo de autor
search-filter-more = Mais filtros
search-filter-more-hint = datas, tipo de autor e mais
search-hint-user = usuário
search-hint-channel = canal
search-hint-has = link, incorporação ou arquivo
search-hint-date = data
search-hint-pinned = true ou false
search-hint-authortype = user, bot ou webhook
search-history = Histórico
search-clear-history = Limpar histórico
search-today = Hoje
search-yesterday = Ontem
search-no-matches = Nenhum resultado
menu-bookmark = Salvar mensagem
menu-reminder = Criar lembrete
menu-reminder-30m = Em 30 minutos
menu-reminder-1h = Em 1 hora
menu-reminder-3h = Em 3 horas
menu-reminder-tomorrow = Amanhã de manhã
menu-speak = Ler mensagem em voz alta
menu-publish = Publicar
title-back = Voltar
title-forward = Avançar
menu-search-image = Pesquisar imagem
menu-copy-image = Copiar imagem
menu-save-image = Salvar imagem
menu-copy-image-link = Copiar link da imagem
menu-open-image-link = Abrir link da imagem
menu-open-link = Abrir link
menu-copy = Copiar
menu-apps = Apps
