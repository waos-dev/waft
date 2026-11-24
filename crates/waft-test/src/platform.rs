//! A platform a test configures and reads back.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use waft_embedder::{
    AppExit, AppLifecycleState, Brightness, Cameras, CaptionSettings, Clipboard, CompositionLayer,
    Compositor, Dispatcher, DockMenu, FileDialogs, Files, FontSource, Haptics, HardwareBuffer,
    HardwareBufferAllocator, HttpClient, Image, ImageCodecFuture, ImageDecodeError, Instant,
    MediaAssets, MenuBar, Notifications, Offset, Opener, Picture, Platform, Players, PointerReport,
    PopupMenus, RequestId, Restoration, Screen, Screens, SignedDuration, SystemChrome,
    SystemContextMenu, TargetPlatform, TextServices, TrayIcons, VideoFrame, VideoFrameReaders,
    View, ViewEmbedder, ViewError, ViewFocusEvent, ViewHost, ViewId, ViewMetrics, ViewRef,
    WindowConfig,
};

use waft_embedder::image_codecs::{ImageCodecs, ImageError, ImageErrorKind, ImageFuture};
use waft_embedder::video_decoders::VideoDecoders;
use waft_embedder::{SystemFontSource, test_support, valo};

use crate::windows::view_of_request;
use crate::{TestFontSource, TestTimeZone, TestView, TestWindows};

type FontSourceMaker = Rc<dyn Fn() -> Box<dyn FontSource>>;
type ImageCodecOpener = Rc<dyn Fn(Arc<[u8]>) -> ImageCodecFuture>;
type PixelImporter = Rc<dyn Fn(valo::PixelBuffer) -> Option<Image>>;
type PictureRasterizer = Rc<dyn Fn(&Picture, u32, u32) -> Option<Image>>;
type ImageReader = Rc<dyn Fn(&Image) -> ImageFuture<valo::PixelBuffer>>;
type FrameImporter = Rc<dyn Fn(&dyn VideoFrame) -> Option<Image>>;
type BufferImporter = Rc<dyn Fn(&dyn HardwareBuffer) -> Option<Image>>;

/// A platform a test builds: which host it claims to be, the views it offers, and each
/// capability as the test's own implementation or absent. It keeps the frames the framework asks
/// for, what it says of whether it handles back, and the deadlines it asks to be woken at, so a
/// test can assert on them; the test itself
/// pumps the app, since there is no loop to run it.
///
/// With a compositor it has a view embedder, which adds a view on any layer the app asks it to
/// (`ViewEmbedder::add_view`), at once: a [`TestView`] at the request's metrics, carrying its id
/// and asking what it asks of the system of the app's host, listed among the views until the app
/// removes it. The test tells the framework of each view's coming and going itself, as a host
/// would through `EmbedderClient::view_added` and `view_removed`.
pub struct TestPlatform {
    target: Cell<TargetPlatform>,
    briefly_show_password: Cell<bool>,
    brightness: Cell<Brightness>,
    time_zone: RefCell<TestTimeZone>,
    default_route: String,
    /// The real instant the platform's clock starts from; [`now`](Platform::now) moves only when
    /// [`advance_clock`](Self::advance_clock) advances [`elapsed`](Self::elapsed).
    clock_origin: Instant,
    /// How far the test has moved the platform's clock since `clock_origin`.
    elapsed: Cell<Duration>,
    /// The screens the platform reports and where the pointer is on them; `None` in both means
    /// [`Platform::screens`] answers `None`.
    screens: TestScreens,
    /// The views the platform gave the app, as a phone gives its screen.
    views: RefCell<Vec<ViewRef>>,
    /// The number the next request for a view takes, a window's or one on a layer.
    next_request: Cell<u64>,
    /// The views the app added on layers and has not removed, in the order it added them.
    added_views: RefCell<Vec<Rc<TestView>>>,
    /// The view of each frame asked for since the test last took them, in the order asked.
    frame_requests: RefCell<Vec<ViewId>>,
    dispatcher: Arc<TestDispatcher>,
    /// Makes the source the platform's fonts come from: flutter_test's unless the test gives
    /// one.
    font_source: FontSourceMaker,
    image_codec: Option<ImageCodecOpener>,
    pixel_import: Option<PixelImporter>,
    picture_to_image: Option<PictureRasterizer>,
    image_read_back: Option<ImageReader>,
    frame_import: Option<FrameImporter>,
    buffer_import: Option<BufferImporter>,
    windows: Option<Rc<TestWindows>>,
    clipboard: Option<Rc<dyn Clipboard>>,
    system_chrome: Option<Rc<dyn SystemChrome>>,
    haptics: Option<Rc<dyn Haptics>>,
    system_context_menu: Option<Rc<dyn SystemContextMenu>>,
    menu_bar: Option<Rc<dyn MenuBar>>,
    popup_menus: Option<Rc<dyn PopupMenus>>,
    tray_icons: Option<Rc<dyn TrayIcons>>,
    file_dialogs: Option<Rc<dyn FileDialogs>>,
    http: Option<Rc<dyn HttpClient>>,
    files: Option<Rc<dyn Files>>,
    opener: Option<Rc<dyn Opener>>,
    notifications: Option<Rc<dyn Notifications>>,
    compositor: Option<Arc<dyn Compositor>>,
    hardware_buffer_allocator: Option<Arc<dyn HardwareBufferAllocator>>,
    players: Option<Rc<dyn Players>>,
    media_assets: Option<Rc<dyn MediaAssets>>,
    image_codecs: Option<Rc<dyn ImageCodecs>>,
    video_decoders: Option<Rc<dyn VideoDecoders>>,
    cameras: Option<Rc<dyn Cameras>>,
    video_frame_readers: Option<Rc<dyn VideoFrameReaders>>,
    caption_settings: Option<Rc<dyn CaptionSettings>>,
    dock_menu: Option<Rc<dyn DockMenu>>,
    text_services: Option<Rc<dyn TextServices>>,
    restoration: Option<Rc<dyn Restoration>>,
    app_exit: Option<Rc<dyn AppExit>>,
    initial_lifecycle_state: Option<AppLifecycleState>,
}

impl TestPlatform {
    /// A platform claiming to be Android, as Flutter's test binding does, in light mode, with
    /// flutter_test's fonts, no view and no capability.
    pub fn new() -> TestPlatform {
        TestPlatform {
            target: Cell::new(TargetPlatform::Android),
            briefly_show_password: Cell::new(true),
            brightness: Cell::new(Brightness::Light),
            time_zone: RefCell::default(),
            default_route: "/".into(),
            clock_origin: Instant::now(),
            elapsed: Cell::new(Duration::ZERO),
            screens: TestScreens::default(),
            views: RefCell::default(),
            next_request: Cell::new(1),
            added_views: RefCell::default(),
            frame_requests: RefCell::default(),
            dispatcher: Arc::new(TestDispatcher::default()),
            font_source: Rc::new(|| Box::new(TestFontSource::new())),
            image_codec: None,
            pixel_import: None,
            picture_to_image: None,
            image_read_back: None,
            frame_import: None,
            buffer_import: None,
            windows: None,
            clipboard: None,
            system_chrome: None,
            haptics: None,
            system_context_menu: None,
            menu_bar: None,
            popup_menus: None,
            tray_icons: None,
            file_dialogs: None,
            http: None,
            files: None,
            opener: None,
            notifications: None,
            compositor: None,
            hardware_buffer_allocator: None,
            players: None,
            media_assets: None,
            image_codecs: None,
            video_decoders: None,
            cameras: None,
            video_frame_readers: None,
            caption_settings: None,
            dock_menu: None,
            text_services: None,
            restoration: None,
            app_exit: None,
            initial_lifecycle_state: None,
        }
    }

    /// The same platform claiming to be `target`.
    pub fn on(self, target: TargetPlatform) -> TestPlatform {
        self.target.set(target);
        self
    }

    /// The same platform giving the app `view`, as a phone gives its screen.
    pub fn with_view(self, view: ViewRef) -> TestPlatform {
        self.views.borrow_mut().push(view);
        self
    }

    /// The same platform giving the app an 800 x 600 [`TestView`] numbered by each of `ids`, in
    /// that order, as a desktop gives one window to each.
    pub fn with_views(self, ids: &[ViewId]) -> TestPlatform {
        for &id in ids {
            let view = Rc::new(TestView::new(800.0, 600.0).with_id(id));
            self.views.borrow_mut().push(view as ViewRef);
        }
        self
    }

    /// The [`TestView`] numbered `id` among the platform's views, however it came: given,
    /// made for a window the app asked for, or added on a layer.
    ///
    /// # Panics
    ///
    /// If none of the platform's views numbered `id` is a [`TestView`].
    pub fn test_view(&self, id: ViewId) -> Rc<TestView> {
        self.views()
            .into_iter()
            .find(|view| view.id() == id)
            .and_then(|view| (view as Rc<dyn std::any::Any>).downcast::<TestView>().ok())
            .unwrap_or_else(|| panic!("no TestView numbered {id:?} among the platform's views"))
    }

    /// How far the test has moved the platform's clock since it was made.
    pub fn elapsed(&self) -> Duration {
        self.elapsed.get()
    }

    /// Advances the platform's clock by `duration`, so [`Platform::now`] moves only when the test
    /// says so.
    pub fn advance_clock(&self, duration: Duration) {
        self.elapsed.set(self.elapsed.get() + duration);
    }

    /// The same platform reporting `screens` from [`Platform::screens`].
    pub fn with_screens(self, screens: Vec<Screen>) -> TestPlatform {
        self.set_screens(screens);
        self
    }

    /// Changes the screens [`Platform::screens`] reports; the client hears of it through
    /// `EmbedderClient::screens_changed`.
    pub fn set_screens(&self, screens: Vec<Screen>) {
        *self.screens.screens.borrow_mut() = Some(screens);
    }

    /// The same platform reporting the pointer at `location` from
    /// [`Screens::pointer_location`].
    pub fn with_pointer_location(self, location: Offset) -> TestPlatform {
        self.set_pointer_location(Some(location));
        self
    }

    /// Changes where [`Screens::pointer_location`] reports the pointer.
    pub fn set_pointer_location(&self, location: Option<Offset>) {
        self.screens.pointer_location.set(location);
    }

    /// The views the app added on layers and has not removed, in the order it added them.
    pub fn added_views(&self) -> Vec<Rc<TestView>> {
        self.added_views.borrow().clone()
    }

    /// Gives the app `view` while it runs, as an embedder adds one; the test tells the
    /// framework of it itself, as a host would through `EmbedderClient::view_added`.
    pub fn give_view(&self, view: ViewRef) {
        self.views.borrow_mut().push(view);
    }

    /// Takes the view numbered `id` back while the app runs.
    pub fn take_view(&self, id: ViewId) {
        self.views.borrow_mut().retain(|view| view.id() != id);
    }

    /// The same platform answering `shown` for the system's setting of whether an obscured field
    /// briefly shows the character just typed; on unless set, as dart:ui's default is.
    pub fn with_briefly_show_password(self, shown: bool) -> TestPlatform {
        self.briefly_show_password.set(shown);
        self
    }

    pub fn with_brightness(self, brightness: Brightness) -> TestPlatform {
        self.brightness.set(brightness);
        self
    }

    /// The route the host would open the app at; `/` unless set.
    pub fn with_default_route(mut self, route: impl Into<String>) -> TestPlatform {
        self.default_route = route.into();
        self
    }

    /// The lifecycle state the host would start the app in; none unless set.
    pub fn with_initial_lifecycle_state(mut self, state: AppLifecycleState) -> TestPlatform {
        self.initial_lifecycle_state = Some(state);
        self
    }

    /// Fonts for text to shape against, in place of flutter_test's, where every family is
    /// FlutterTest ([`TestFontSource`]).
    pub fn with_font_source(
        mut self,
        make: impl Fn() -> Box<dyn FontSource> + 'static,
    ) -> TestPlatform {
        self.font_source = Rc::new(make);
        self
    }

    /// The system's fonts in place of flutter_test's, as a host shapes text against, for a test
    /// that looks at what its text draws as.
    pub fn with_system_fonts(self) -> TestPlatform {
        self.with_font_source(|| {
            Box::new(SystemFontSource::new(valo_system_fonts::fontmgr::platform()))
        })
    }

    /// One GPU with no window, as a host has its renderer's: encoded pictures, such as a PNG's
    /// bytes, are opened on it, images made from pixels and pictures, and images read back. What
    /// a test that looks at what the app paints gives its platform. Encoded pictures are decoded
    /// by valo's software decoders, the same on every machine.
    pub fn with_gpu(self) -> TestPlatform {
        self.with_image_codec(test_support::open_image_codec)
            .with_pixel_import(test_support::imported_pixels)
            .with_picture_to_image(|picture, width, height| {
                Some(test_support::picture_to_image(picture, [width, height]))
            })
            .with_read_image_pixels(test_support::read_image_pixels)
    }

    /// The decoder behind `open_image_codec`; none unless set, so every image fails to open.
    pub fn with_image_codec(
        mut self,
        open: impl Fn(Arc<[u8]>) -> ImageCodecFuture + 'static,
    ) -> TestPlatform {
        self.image_codec = Some(Rc::new(open));
        self
    }

    /// The renderer behind `import_pixels`; none unless set, so no image is made from pixels.
    pub fn with_pixel_import(
        mut self,
        import: impl Fn(valo::PixelBuffer) -> Option<Image> + 'static,
    ) -> TestPlatform {
        self.pixel_import = Some(Rc::new(import));
        self
    }

    /// The renderer behind `picture_to_image_sync`; none unless set, so no picture is drawn
    /// into an image.
    pub fn with_picture_to_image(
        mut self,
        draw: impl Fn(&Picture, u32, u32) -> Option<Image> + 'static,
    ) -> TestPlatform {
        self.picture_to_image = Some(Rc::new(draw));
        self
    }

    /// The renderer behind `read_image_pixels`; none unless set, so no image is read back.
    pub fn with_read_image_pixels(
        mut self,
        read: impl Fn(&Image) -> ImageFuture<valo::PixelBuffer> + 'static,
    ) -> TestPlatform {
        self.image_read_back = Some(Rc::new(read));
        self
    }

    /// The renderer behind `import_video_frame`; none unless set, so no image is made of a frame.
    pub fn with_frame_import(
        mut self,
        import: impl Fn(&dyn VideoFrame) -> Option<Image> + 'static,
    ) -> TestPlatform {
        self.frame_import = Some(Rc::new(import));
        self
    }

    /// The renderer behind `import_hardware_buffer`; none unless set, so no image is made of a
    /// buffer.
    pub fn with_buffer_import(
        mut self,
        import: impl Fn(&dyn HardwareBuffer) -> Option<Image> + 'static,
    ) -> TestPlatform {
        self.buffer_import = Some(Rc::new(import));
        self
    }

    /// The same platform answering requests for views with `windows`, as a desktop makes a
    /// window for each.
    pub fn with_windows(mut self, windows: Rc<TestWindows>) -> TestPlatform {
        self.windows = Some(windows);
        self
    }

    pub fn with_clipboard(mut self, clipboard: Rc<dyn Clipboard>) -> TestPlatform {
        self.clipboard = Some(clipboard);
        self
    }

    pub fn with_system_chrome(mut self, system_chrome: Rc<dyn SystemChrome>) -> TestPlatform {
        self.system_chrome = Some(system_chrome);
        self
    }

    pub fn with_haptics(mut self, haptics: Rc<dyn Haptics>) -> TestPlatform {
        self.haptics = Some(haptics);
        self
    }

    pub fn with_system_context_menu(mut self, menu: Rc<dyn SystemContextMenu>) -> TestPlatform {
        self.system_context_menu = Some(menu);
        self
    }

    pub fn with_menu_bar(mut self, menu_bar: Rc<dyn MenuBar>) -> TestPlatform {
        self.menu_bar = Some(menu_bar);
        self
    }

    pub fn with_popup_menus(mut self, menus: Rc<dyn PopupMenus>) -> TestPlatform {
        self.popup_menus = Some(menus);
        self
    }

    pub fn with_tray_icons(mut self, tray_icons: Rc<dyn TrayIcons>) -> TestPlatform {
        self.tray_icons = Some(tray_icons);
        self
    }

    pub fn with_file_dialogs(mut self, file_dialogs: Rc<dyn FileDialogs>) -> TestPlatform {
        self.file_dialogs = Some(file_dialogs);
        self
    }

    /// The HTTP client behind `http`; none unless set, so every request fails at once.
    pub fn with_http(mut self, http: Rc<dyn HttpClient>) -> TestPlatform {
        self.http = Some(http);
        self
    }

    /// The file system behind `files`; none unless set, so every path fails at once.
    pub fn with_files(mut self, files: Rc<dyn Files>) -> TestPlatform {
        self.files = Some(files);
        self
    }

    pub fn with_opener(mut self, opener: Rc<dyn Opener>) -> TestPlatform {
        self.opener = Some(opener);
        self
    }

    pub fn with_notifications(mut self, notifications: Rc<dyn Notifications>) -> TestPlatform {
        self.notifications = Some(notifications);
        self
    }

    pub fn with_compositor(mut self, compositor: Arc<dyn Compositor>) -> TestPlatform {
        self.compositor = Some(compositor);
        self
    }

    pub fn with_hardware_buffer_allocator(
        mut self,
        allocator: Arc<dyn HardwareBufferAllocator>,
    ) -> TestPlatform {
        self.hardware_buffer_allocator = Some(allocator);
        self
    }

    pub fn with_players(mut self, players: Rc<dyn Players>) -> TestPlatform {
        self.players = Some(players);
        self
    }

    pub fn with_media_assets(mut self, media_assets: Rc<dyn MediaAssets>) -> TestPlatform {
        self.media_assets = Some(media_assets);
        self
    }

    pub fn with_image_codecs(mut self, image_codecs: Rc<dyn ImageCodecs>) -> TestPlatform {
        self.image_codecs = Some(image_codecs);
        self
    }

    /// The platform answering `video_decoders` as its video decoders: a host's own, where a test
    /// decodes for real, as `waft-embedder-apple`'s are VideoToolbox's.
    pub fn with_video_decoders(mut self, video_decoders: Rc<dyn VideoDecoders>) -> TestPlatform {
        self.video_decoders = Some(video_decoders);
        self
    }

    pub fn with_cameras(mut self, cameras: Rc<dyn Cameras>) -> TestPlatform {
        self.cameras = Some(cameras);
        self
    }

    pub fn with_video_frame_readers(
        mut self,
        video_frame_readers: Rc<dyn VideoFrameReaders>,
    ) -> TestPlatform {
        self.video_frame_readers = Some(video_frame_readers);
        self
    }

    pub fn with_caption_settings(
        mut self,
        caption_settings: Rc<dyn CaptionSettings>,
    ) -> TestPlatform {
        self.caption_settings = Some(caption_settings);
        self
    }

    pub fn with_dock_menu(mut self, dock_menu: Rc<dyn DockMenu>) -> TestPlatform {
        self.dock_menu = Some(dock_menu);
        self
    }

    pub fn with_text_services(mut self, services: Rc<dyn TextServices>) -> TestPlatform {
        self.text_services = Some(services);
        self
    }

    pub fn with_restoration(mut self, restoration: Rc<dyn Restoration>) -> TestPlatform {
        self.restoration = Some(restoration);
        self
    }

    pub fn with_app_exit(mut self, app_exit: Rc<dyn AppExit>) -> TestPlatform {
        self.app_exit = Some(app_exit);
        self
    }

    /// The platform in `zone`, which is UTC unless a test says otherwise.
    pub fn with_time_zone(self, zone: TestTimeZone) -> TestPlatform {
        self.set_time_zone(zone);
        self
    }

    /// Changes which host the platform claims to be, for a test that switches mid-way.
    pub fn set_target(&self, target: TargetPlatform) {
        self.target.set(target);
    }

    /// Changes whether an obscured field briefly shows the character just typed, as the user
    /// changes the system's setting; dart:ui tells no one, and the framework reads it next as
    /// it needs it.
    pub fn set_briefly_show_password(&self, shown: bool) {
        self.briefly_show_password.set(shown);
    }

    /// Changes the local time zone, as the user changes the system's; a `DateTime` made after it
    /// reads the new zone.
    pub fn set_time_zone(&self, zone: TestTimeZone) {
        *self.time_zone.borrow_mut() = zone;
    }

    /// Changes the brightness; the client hears of it as it would from a host.
    pub fn set_brightness(&self, brightness: Brightness) {
        self.brightness.set(brightness);
    }

    /// The view of each frame the framework asked for since the test last took them, in the
    /// order asked, a view asked again included.
    pub fn frame_requests(&self) -> Vec<ViewId> {
        self.frame_requests.borrow().clone()
    }

    /// Whether `view` asked for a frame since the test last took the requests.
    pub fn has_frame_request(&self, view: ViewId) -> bool {
        self.frame_requests.borrow().contains(&view)
    }

    /// Takes the request `view` made for a frame, for the test to give it its frame; answers
    /// whether it had made one since the last take.
    pub fn take_frame_request(&self, view: ViewId) -> bool {
        let mut requests = self.frame_requests.borrow_mut();
        let asked = requests.contains(&view);
        requests.retain(|&asking| asking != view);
        asked
    }

    /// The views that asked for a frame since the last take, each once, in the order the platform
    /// gained them, for the test to give them a refresh, as a host does. A request of a view the
    /// platform does not have is dropped, as a host drops it.
    pub fn take_frame_requests(&self) -> Vec<ViewId> {
        let requests = std::mem::take(&mut *self.frame_requests.borrow_mut());
        self.views()
            .iter()
            .map(|view| view.id())
            .filter(|id| requests.contains(id))
            .collect()
    }

    /// The timer wakeups the framework has set, in order. A `None` is the framework
    /// saying no timer is waiting.
    pub fn wakes(&self) -> Vec<Option<Instant>> {
        self.dispatcher.timer_wakeups()
    }

    /// How often a ready task has asked the host to wake the app.
    pub fn wake_now_requests(&self) -> usize {
        self.dispatcher.wake_now_requests()
    }

    /// The host's event loop asleep: waits up to `timeout` until a ready task has asked to wake
    /// the app more than `since` times ([`wake_now_requests`](Self::wake_now_requests)), as work
    /// finishing on another thread does.
    pub fn wait_for_wake(&self, since: usize, timeout: Duration) {
        self.dispatcher.wait_for_wake(since, timeout);
    }

    /// The number of a new request for a view.
    fn take_request_id(&self) -> RequestId {
        let id = RequestId(self.next_request.get());
        self.next_request.set(id.0 + 1);
        id
    }
}

impl Default for TestPlatform {
    fn default() -> TestPlatform {
        TestPlatform::new()
    }
}

impl Platform for TestPlatform {
    fn target_platform(&self) -> TargetPlatform {
        self.target.get()
    }

    fn request_frame(&self, view: ViewId) {
        self.frame_requests.borrow_mut().push(view);
    }

    fn now(&self) -> Instant {
        self.clock_origin + self.elapsed.get()
    }

    fn dispatcher(&self) -> Arc<dyn Dispatcher> {
        Arc::clone(&self.dispatcher) as Arc<dyn Dispatcher>
    }

    fn views(&self) -> Vec<ViewRef> {
        let windows = self.windows.iter().flat_map(|windows| windows.open_views());
        let added = self.added_views().into_iter().map(|view| view as ViewRef);
        let given = self.views.borrow().clone();
        given.into_iter().chain(windows).chain(added).collect()
    }

    fn view(&self, id: ViewId) -> Option<ViewRef> {
        self.views().into_iter().find(|view| view.id() == id)
    }

    fn request_view(
        &self,
        config: WindowConfig,
        initial_data: Option<String>,
    ) -> Result<RequestId, ViewError> {
        let windows = self.windows.as_ref().ok_or(ViewError::Unsupported)?;
        let id = self.take_request_id();
        windows.request(id, config, initial_data);
        Ok(id)
    }

    fn briefly_show_password(&self) -> bool {
        self.briefly_show_password.get()
    }

    fn time_zone_offset(&self, seconds_since_epoch: i64) -> SignedDuration {
        self.time_zone.borrow().offset(seconds_since_epoch)
    }

    fn time_zone_name(&self, seconds_since_epoch: i64) -> String {
        self.time_zone.borrow().name(seconds_since_epoch)
    }

    fn platform_brightness(&self) -> Brightness {
        self.brightness.get()
    }

    fn default_route_name(&self) -> String {
        self.default_route.clone()
    }

    fn initial_lifecycle_state(&self) -> Option<AppLifecycleState> {
        self.initial_lifecycle_state
    }

    fn font_source(&self) -> Option<Box<dyn FontSource>> {
        Some((self.font_source)())
    }

    fn open_image_codec(&self, bytes: Arc<[u8]>) -> ImageCodecFuture {
        match &self.image_codec {
            Some(open) => open(bytes),
            None => Box::pin(std::future::ready(Err(ImageDecodeError::NoDecoder))),
        }
    }

    fn import_pixels(&self, pixels: valo::PixelBuffer) -> Option<Image> {
        self.pixel_import.as_ref().and_then(|import| import(pixels))
    }

    fn picture_to_image_sync(&self, picture: &Picture, width: u32, height: u32) -> Option<Image> {
        self.picture_to_image
            .as_ref()
            .and_then(|draw| draw(picture, width, height))
    }

    fn read_image_pixels(&self, image: &Image) -> ImageFuture<valo::PixelBuffer> {
        match &self.image_read_back {
            Some(read) => read(image),
            None => Box::pin(std::future::ready(Err(ImageError::new(
                ImageErrorKind::Unsupported,
                "the test platform reads no image back: give it with_gpu",
            )))),
        }
    }

    fn import_video_frame(&self, frame: &dyn VideoFrame) -> Option<Image> {
        self.frame_import.as_ref().and_then(|import| import(frame))
    }

    fn import_hardware_buffer(&self, buffer: &dyn HardwareBuffer) -> Option<Image> {
        self.buffer_import
            .as_ref()
            .and_then(|import| import(buffer))
    }

    /// None: this host's buffers are in memory, on no GPU, as buffers a device cannot take, which
    /// a drawing skips.
    fn import_texture(
        &self,
        _buffer: &dyn HardwareBuffer,
        _device: &wgpu::Device,
    ) -> Option<wgpu::Texture> {
        None
    }

    fn screens(&self) -> Option<&dyn Screens> {
        (self.screens.screens.borrow().is_some() || self.screens.pointer_location.get().is_some())
            .then_some(&self.screens as &dyn Screens)
    }

    fn clipboard(&self) -> Option<&dyn Clipboard> {
        self.clipboard.as_deref()
    }

    fn system_chrome(&self) -> Option<&dyn SystemChrome> {
        self.system_chrome.as_deref()
    }

    fn haptics(&self) -> Option<&dyn Haptics> {
        self.haptics.as_deref()
    }

    fn system_context_menu(&self) -> Option<&dyn SystemContextMenu> {
        self.system_context_menu.as_deref()
    }

    fn menu_bar(&self) -> Option<&dyn MenuBar> {
        self.menu_bar.as_deref()
    }

    fn popup_menus(&self) -> Option<&dyn PopupMenus> {
        self.popup_menus.as_deref()
    }

    fn tray_icons(&self) -> Option<&dyn TrayIcons> {
        self.tray_icons.as_deref()
    }

    fn file_dialogs(&self) -> Option<&dyn FileDialogs> {
        self.file_dialogs.as_deref()
    }

    fn http(&self) -> Option<&dyn HttpClient> {
        self.http.as_deref()
    }

    fn files(&self) -> Option<&dyn Files> {
        self.files.as_deref()
    }

    fn opener(&self) -> Option<&dyn Opener> {
        self.opener.as_deref()
    }

    fn notifications(&self) -> Option<&dyn Notifications> {
        self.notifications.as_deref()
    }

    fn compositor(&self) -> Option<Arc<dyn Compositor>> {
        self.compositor.clone()
    }

    fn hardware_buffer_allocator(&self) -> Option<Arc<dyn HardwareBufferAllocator>> {
        self.hardware_buffer_allocator.clone()
    }

    fn players(&self) -> Option<&dyn Players> {
        self.players.as_deref()
    }

    fn media_assets(&self) -> Option<&dyn MediaAssets> {
        self.media_assets.as_deref()
    }

    fn image_codecs(&self) -> Option<&dyn ImageCodecs> {
        self.image_codecs.as_deref()
    }

    fn video_decoders(&self) -> Option<&dyn VideoDecoders> {
        self.video_decoders.as_deref()
    }

    fn cameras(&self) -> Option<&dyn Cameras> {
        self.cameras.as_deref()
    }

    fn video_frame_readers(&self) -> Option<&dyn VideoFrameReaders> {
        self.video_frame_readers.as_deref()
    }

    fn caption_settings(&self) -> Option<&dyn CaptionSettings> {
        self.caption_settings.as_deref()
    }

    fn view_embedder(&self) -> Option<&dyn ViewEmbedder> {
        self.compositor.as_ref().map(|_| self as &dyn ViewEmbedder)
    }

    fn dock_menu(&self) -> Option<&dyn DockMenu> {
        self.dock_menu.as_deref()
    }

    fn text_services(&self) -> Option<&dyn TextServices> {
        self.text_services.as_deref()
    }

    fn restoration(&self) -> Option<&dyn Restoration> {
        self.restoration.as_deref()
    }

    fn app_exit(&self) -> Option<&dyn AppExit> {
        self.app_exit.as_deref()
    }
}

/// The views the app adds on layers, made at once. The metrics, sight and input the app sends them
/// go nowhere: a test hands the framework what a host would make of them itself.
impl ViewEmbedder for TestPlatform {
    fn add_view(
        &self,
        _layer: &Arc<dyn CompositionLayer>,
        metrics: ViewMetrics,
        host: Rc<dyn ViewHost>,
        initial_data: Option<String>,
    ) -> Result<RequestId, ViewError> {
        let id = self.take_request_id();
        let [width, height] = metrics.physical_size;
        let mut view = TestView::with_pixel_ratio(width, height, metrics.device_pixel_ratio)
            .with_id(view_of_request(id))
            .with_request(id, initial_data)
            .with_host(host);
        if let Some(screen) = metrics.screen {
            view = view.on_screen(screen);
        }
        self.added_views.borrow_mut().push(Rc::new(view));
        Ok(id)
    }

    fn set_view_metrics(&self, _view: ViewId, _metrics: ViewMetrics) {}

    fn set_view_in_sight(&self, _view: ViewId, _in_sight: bool) {}

    fn remove_view(&self, view: ViewId) {
        self.added_views
            .borrow_mut()
            .retain(|added| added.id() != view);
    }

    fn send_pointer_reports(&self, _reports: Vec<PointerReport>) {}

    fn send_view_focus_event(&self, _event: ViewFocusEvent) {}
}

#[derive(Default)]
struct TestScreens {
    screens: RefCell<Option<Vec<Screen>>>,
    pointer_location: Cell<Option<Offset>>,
}

impl Screens for TestScreens {
    fn screens(&self) -> Vec<Screen> {
        self.screens.borrow().clone().unwrap_or_default()
    }

    fn pointer_location(&self) -> Option<Offset> {
        self.pointer_location.get()
    }
}

/// A host that records what it was asked for and never wakes the app. The test pumps the app
/// instead, where a host would call `EmbedderClient::wake`, and work runs at once on the
/// calling thread, so a test needs only a checkpoint to see its result.
#[derive(Default)]
pub struct TestDispatcher {
    timer_wakeups: Mutex<Vec<Option<Instant>>>,
    /// How often a ready task has asked the host to wake the app.
    wakes: Mutex<usize>,
    /// Told at each wake.
    woken: Condvar,
}

impl TestDispatcher {
    /// The timer wakeups set, in order. A `None` is the application saying no timer is
    /// waiting.
    pub fn timer_wakeups(&self) -> Vec<Option<Instant>> {
        self.timer_wakeups
            .lock()
            .expect("the wakeups are never poisoned: nothing runs under their lock")
            .clone()
    }

    /// How often a ready task has asked the host to wake the app.
    pub fn wake_now_requests(&self) -> usize {
        *self.wakes()
    }

    /// Waits up to `timeout` until a ready task has asked to wake the app more than `since`
    /// times.
    pub(crate) fn wait_for_wake(&self, since: usize, timeout: Duration) {
        let wakes = self.wakes();
        let _ = self
            .woken
            .wait_timeout_while(wakes, timeout, |wakes| *wakes <= since);
    }

    fn wakes(&self) -> std::sync::MutexGuard<'_, usize> {
        self.wakes
            .lock()
            .expect("the wake counter is never poisoned: nothing runs under its lock")
    }
}

impl Dispatcher for TestDispatcher {
    fn wake_at(&self, timer_wakeup: Option<Instant>) {
        self.timer_wakeups
            .lock()
            .expect("the wakeups are never poisoned: nothing runs under their lock")
            .push(timer_wakeup);
    }

    fn wake_now(&self) {
        *self.wakes() += 1;
        self.woken.notify_all();
    }

    fn dispatch(&self, work: Box<dyn FnOnce() + Send>) {
        work();
    }
}

#[cfg(test)]
mod tests {
    use waft_embedder::Platform;

    use super::TestPlatform;

    #[test]
    fn the_test_platforms_fonts_are_flutter_test() {
        let platform = TestPlatform::new();
        let mut source = platform.font_source().expect("the test platform has fonts");
        assert!(source.family("Roboto")[0].matches("FlutterTest"));
    }

    #[test]
    fn with_the_systems_fonts_a_family_is_the_systems_not_flutter_tests() {
        let platform = TestPlatform::new().with_system_fonts();
        let mut source = platform.font_source().expect("the test platform has fonts");
        let face = source
            .face_for_codepoint('a', Default::default())
            .expect("the system has a face for 'a'");
        assert!(!face.matches("FlutterTest"));
    }

    #[test]
    fn with_gpu_a_png_opens_as_its_picture() {
        use waft_embedder::valo;
        let pixels = valo::PixelBuffer::new(
            valo::PixelLayout::packed([3, 2], valo::PixelFormat::Rgba8, valo::AlphaType::Straight),
            vec![255; 3 * 2 * 4],
        )
        .expect("white pixels");
        let png = crate::encode_png(&pixels);
        let platform = TestPlatform::new().with_gpu();
        let mut codec =
            pollster::block_on(platform.open_image_codec(png.into())).expect("a PNG opens");
        assert_eq!(codec.frame_count(), 1);
        let frame = pollster::block_on(codec.next_frame()).expect("its frame");
        assert_eq!(frame.image.size(), [3, 2]);
    }

    #[test]
    fn a_test_platform_briefly_shows_passwords_unless_told_not_to() {
        assert!(TestPlatform::new().briefly_show_password());
        assert!(
            !TestPlatform::new()
                .with_briefly_show_password(false)
                .briefly_show_password()
        );
    }

    #[test]
    fn a_change_of_the_setting_is_what_the_platform_answers_next() {
        let platform = TestPlatform::new();
        platform.set_briefly_show_password(false);
        assert!(!platform.briefly_show_password());
        platform.set_briefly_show_password(true);
        assert!(platform.briefly_show_password());
    }

    #[test]
    fn the_clock_moves_only_when_advanced() {
        use std::time::Duration;

        let platform = TestPlatform::new();
        let start = platform.now();
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(platform.now(), start);
        assert_eq!(platform.elapsed(), Duration::ZERO);

        platform.advance_clock(Duration::from_millis(16));
        assert_eq!(platform.elapsed(), Duration::from_millis(16));
        assert_eq!(
            platform.now().duration_since(start),
            Duration::from_millis(16)
        );

        platform.advance_clock(Duration::from_millis(4));
        assert_eq!(platform.elapsed(), Duration::from_millis(20));
        assert_eq!(
            platform.now().duration_since(start),
            Duration::from_millis(20)
        );
    }

    #[test]
    fn screens_and_pointer_location_are_absent_until_configured() {
        use waft_embedder::{Offset, Rect, Screen, ScreenId};

        let bare = TestPlatform::new();
        assert!(bare.screens().is_none());

        let main = Screen {
            id: ScreenId(1),
            name: "Main".to_owned(),
            frame: Rect::from_ltwh(0.0, 0.0, 1440.0, 900.0),
            visible_frame: Rect::from_ltwh(0.0, 25.0, 1440.0, 875.0),
            scale: 2.0,
            refresh_rate: Some(60.0),
        };
        let with_screens = TestPlatform::new()
            .with_screens(vec![main.clone()])
            .with_pointer_location(Offset::new(120.0, 340.0));
        let screens = with_screens.screens().expect("configured screens");
        assert_eq!(screens.screens(), [main]);
        assert_eq!(screens.pointer_location(), Some(Offset::new(120.0, 340.0)));

        with_screens.set_pointer_location(None);
        assert_eq!(with_screens.screens().unwrap().pointer_location(), None);

        let pointer_only = TestPlatform::new().with_pointer_location(Offset::new(5.0, 6.0));
        let screens = pointer_only.screens().expect("pointer location configured");
        assert!(screens.screens().is_empty());
        assert_eq!(screens.pointer_location(), Some(Offset::new(5.0, 6.0)));
    }
}
