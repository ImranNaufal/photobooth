using System;
using System.Collections.Generic;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Windows.Forms;
using Canon.Eos.Framework;
using Canon.Eos.Framework.Eventing;

namespace CanonBridge
{
    internal static class Program
    {
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool SetDllDirectory(string lpPathName);

        private static readonly object _lvLock = new object();
        private static byte[] _latestLvJpeg = null;
        private static DateTime _latestLvUtc = DateTime.MinValue;

        private static readonly object _capLock = new object();
        private static ManualResetEventSlim _captureDone = null;
        private static string _captureTargetPath = null;
        private static string _captureError = null;
        private static bool _captureInProgress = false;

        private static Control _staInvoker = null;
        private static EosFramework _framework = null;
        private static EosCamera _camera = null;
        private static string _cameraModel = "";
        private static string _lastError = "";
        private static bool _wantLiveView = false;
        private static string _tempSaveDir = Path.Combine(Path.GetTempPath(), "PhotoboothCanonTemp");

        private static TcpListener _listener = null;
        private static volatile bool _running = true;

        private static void Log(string msg)
        {
            try
            {
                string logDir = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "logs");
                Directory.CreateDirectory(logDir);
                string line = DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss.fff") + " " + msg + Environment.NewLine;
                File.AppendAllText(Path.Combine(logDir, "canon_bridge.log"), line);
            }
            catch { }
        }

        private static bool _lvDiagLogged = false;
        private static int _lvErrorCount = 0;

        [STAThread]
        private static void Main(string[] args)
        {
            ConfigureNativeAndAssemblySearchPaths();
            Directory.CreateDirectory(_tempSaveDir);

            int port = 5513;
            int parentPid = 0;
            for (int i = 0; i < args.Length; i++)
            {
                if (args[i] == "--port" && i + 1 < args.Length)
                {
                    int.TryParse(args[i + 1], out port);
                }
                else if (args[i] == "--parent-pid" && i + 1 < args.Length)
                {
                    int.TryParse(args[i + 1], out parentPid);
                }
            }

            Log("Starting CanonBridge on port " + port + " (parentPid=" + parentPid + ")");

            // Invisible Win32 message-pump Control on the STA thread
            _staInvoker = new Control();
            IntPtr _forceHandle = _staInvoker.Handle;

            try
            {
                _framework = new EosFramework();
                _framework.CameraAdded += delegate
                {
                    if (!_captureInProgress) SafeStaInvoke(TryConnectCamera);
                };
                TryConnectCamera();
            }
            catch (Exception ex)
            {
                _lastError = "Framework init error: " + ex.Message;
                Log(_lastError);
            }

            // Reconnect watchdog timer on STA thread (1500ms) - NEVER reconnect during CLI capture!
            var timer = new System.Windows.Forms.Timer();
            timer.Interval = 1500;
            timer.Tick += delegate
            {
                if (_captureInProgress) return;
                if (_camera == null || !_camera.IsSessionOpen)
                {
                    TryConnectCamera();
                }
            };
            timer.Start();

            // Active EVF Live View polling timer on STA thread (~28 FPS)
            var lvTimer = new System.Windows.Forms.Timer();
            lvTimer.Interval = 35;
            lvTimer.Tick += delegate
            {
                if (_wantLiveView && !_captureInProgress && _camera != null && _camera.IsSessionOpen)
                {
                    try
                    {
                        if (!_camera.IsInHostLiveViewMode)
                        {
                            _camera.StartLiveView();
                            if (!_camera.IsInHostLiveViewMode)
                            {
                                _camera.LiveViewDevice = EosLiveViewDevice.Host;
                            }
                        }
                        bool ok = _camera.DownloadEvf();
                        if (!_lvDiagLogged)
                        {
                            _lvDiagLogged = true;
                            Log("LV Poll diag: Device=" + _camera.LiveViewDevice + " IsInLV=" + _camera.IsInLiveViewMode + " IsHost=" + _camera.IsInHostLiveViewMode + " DownloadEvf=" + ok);
                        }
                    }
                    catch (Exception ex)
                    {
                        _lvErrorCount++;
                        if (_lvErrorCount <= 5)
                        {
                            Log("LV Poll exception #" + _lvErrorCount + ": " + ex.Message);
                        }
                    }
                }
            };
            lvTimer.Start();

            // Watch parent process if specified so bridge never orphans
            if (parentPid > 0)
            {
                var parentThread = new Thread(delegate()
                {
                    try
                    {
                        var proc = System.Diagnostics.Process.GetProcessById(parentPid);
                        proc.WaitForExit();
                    }
                    catch { }
                    ShutdownApp();
                });
                parentThread.IsBackground = true;
                parentThread.Start();
            }

            // Start direct TcpListener HTTP server (bypasses Windows http.sys completely)
            var httpThread = new Thread(delegate() { RunTcpHttpServer(port); });
            httpThread.IsBackground = true;
            httpThread.Start();

            Application.Run();
        }

        private static void ConfigureNativeAndAssemblySearchPaths()
        {
            string baseDir = AppDomain.CurrentDomain.BaseDirectory;
            string[] candidates = new string[]
            {
                baseDir,
                Path.Combine(baseDir, "lib"),
                @"C:\Program Files (x86)\digiCamControl",
                @"C:\Program Files\digiCamControl"
            };

            foreach (string dir in candidates)
            {
                if (File.Exists(Path.Combine(dir, "EDSDK.dll")))
                {
                    SetDllDirectory(dir);
                    break;
                }
            }

            AppDomain.CurrentDomain.AssemblyResolve += delegate(object sender, ResolveEventArgs e)
            {
                string shortName = new AssemblyName(e.Name).Name + ".dll";
                foreach (string dir in candidates)
                {
                    string path = Path.Combine(dir, shortName);
                    if (File.Exists(path))
                    {
                        return Assembly.LoadFrom(path);
                    }
                }
                return null;
            };
        }

        private static void SafeStaInvoke(Action action)
        {
            if (_staInvoker != null && !_staInvoker.IsDisposed)
            {
                try
                {
                    if (_staInvoker.InvokeRequired)
                        _staInvoker.Invoke(action);
                    else
                        action();
                }
                catch { }
            }
        }

        private static void TryConnectCamera()
        {
            if (_framework == null) return;
            try
            {
                if (_camera != null)
                {
                    if (_camera.IsSessionOpen) return;
                    CleanupCamera();
                }

                EosCamera cam = _framework.GetCamera();
                if (cam == null)
                {
                    _cameraModel = "";
                    return;
                }

                cam.IsErrorTolerantMode = false;
                cam.EnsureOpenSession();
                try
                {
                    cam.SavePicturesToHost(_tempSaveDir);
                    Log("SavePicturesToHost OK: " + _tempSaveDir);
                }
                catch (Exception spEx)
                {
                    Log("SavePicturesToHost WARN: " + spEx.Message);
                }
                cam.IsErrorTolerantMode = true;
                cam.LiveViewUpdate += OnLiveViewUpdate;
                cam.PictureTaken += OnPictureTaken;
                cam.Error += OnCameraError;
                cam.Shutdown += OnCameraShutdown;

                _camera = cam;
                _cameraModel = cam.DeviceDescription ?? "Canon EOS";
                _lastError = "";
                Log("Connected camera: " + _cameraModel);

                if (_wantLiveView && !cam.IsInLiveViewMode)
                {
                    try { cam.StartLiveView(); } catch (Exception ex) { _lastError = ex.Message; }
                }
            }
            catch (Exception ex)
            {
                _lastError = ex.Message;
                CleanupCamera();
            }
        }

        private static void CleanupCamera()
        {
            if (_camera != null)
            {
                try { _camera.LiveViewUpdate -= OnLiveViewUpdate; } catch { }
                try { _camera.PictureTaken -= OnPictureTaken; } catch { }
                try { _camera.Error -= OnCameraError; } catch { }
                try { _camera.Shutdown -= OnCameraShutdown; } catch { }
                try { _camera.Dispose(); } catch { }
                _camera = null;
            }
            _cameraModel = "";
            lock (_lvLock)
            {
                _latestLvJpeg = null;
            }
        }

        private static void OnLiveViewUpdate(object sender, EosLiveImageEventArgs e)
        {
            try
            {
                byte[] data = e.ImageData;
                if (data == null || data.Length == 0)
                {
                    using (Stream s = e.GetStream())
                    {
                        if (s != null)
                        {
                            using (var ms = new MemoryStream())
                            {
                                s.CopyTo(ms);
                                data = ms.ToArray();
                            }
                        }
                    }
                }

                if (data != null && data.Length > 100)
                {
                    bool firstFrame = false;
                    lock (_lvLock)
                    {
                        if (_latestLvJpeg == null) firstFrame = true;
                        _latestLvJpeg = data;
                        _latestLvUtc = DateTime.UtcNow;
                    }
                    if (firstFrame)
                    {
                        Log("First LiveView EVF JPEG frame received (" + data.Length + " bytes)");
                    }
                }
            }
            catch { }
        }

        private static void OnPictureTaken(object sender, EosImageEventArgs e)
        {
            try
            {
                string dest = _captureTargetPath;
                if (!string.IsNullOrEmpty(dest))
                {
                    string parent = Path.GetDirectoryName(dest);
                    if (!string.IsNullOrEmpty(parent)) Directory.CreateDirectory(parent);

                    var fileArgs = e as EosFileImageEventArgs;
                    var memArgs = e as EosMemoryImageEventArgs;

                    if (fileArgs != null && !string.IsNullOrEmpty(fileArgs.ImageFilePath) && File.Exists(fileArgs.ImageFilePath))
                    {
                        File.Copy(fileArgs.ImageFilePath, dest, true);
                        try { File.Delete(fileArgs.ImageFilePath); } catch { }
                    }
                    else if (memArgs != null && memArgs.ImageData != null && memArgs.ImageData.Length > 0)
                    {
                        File.WriteAllBytes(dest, memArgs.ImageData);
                    }
                    else
                    {
                        using (Stream s = e.GetStream())
                        {
                            if (s != null)
                            {
                                using (var fs = new FileStream(dest, FileMode.Create, FileAccess.Write))
                                {
                                    s.CopyTo(fs);
                                }
                            }
                            else
                            {
                                using (var img = e.GetImage())
                                {
                                    img.Save(dest, System.Drawing.Imaging.ImageFormat.Jpeg);
                                }
                            }
                        }
                    }
                    Log("Saved captured DSLR photo to: " + dest);
                }
                _captureError = null;
            }
            catch (Exception ex)
            {
                _captureError = ex.Message;
                Log("OnPictureTaken error: " + ex.Message);
            }
            finally
            {
                var evt = _captureDone;
                if (evt != null) evt.Set();
            }
        }

        private static void OnCameraError(object sender, EosExceptionEventArgs e)
        {
            if (e != null && e.Exception != null)
            {
                _lastError = e.Exception.Message;
                Log("Camera error: " + _lastError);
            }
        }

        private static void OnCameraShutdown(object sender, EventArgs e)
        {
            Log("Camera shutdown event received");
            CleanupCamera();
        }

        private static void RunTcpHttpServer(int port)
        {
            try
            {
                _listener = new TcpListener(IPAddress.Loopback, port);
                _listener.Start();
                Log("TcpListener HTTP server listening on 127.0.0.1:" + port);
            }
            catch (Exception ex)
            {
                _lastError = "TcpListener start error: " + ex.Message;
                Log(_lastError);
                return;
            }

            while (_running)
            {
                try
                {
                    TcpClient client = _listener.AcceptTcpClient();
                    ThreadPool.QueueUserWorkItem(delegate(object state)
                    {
                        HandleTcpClient((TcpClient)state);
                    }, client);
                }
                catch
                {
                    if (!_running) break;
                }
            }
        }

        private static Dictionary<string, string> ParseQuery(string queryString)
        {
            var dict = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            if (string.IsNullOrEmpty(queryString)) return dict;
            string[] pairs = queryString.Split('&');
            foreach (string pair in pairs)
            {
                int eq = pair.IndexOf('=');
                if (eq > 0)
                {
                    string k = Uri.UnescapeDataString(pair.Substring(0, eq).Replace('+', ' '));
                    string v = Uri.UnescapeDataString(pair.Substring(eq + 1).Replace('+', ' '));
                    dict[k] = v;
                }
            }
            return dict;
        }

        private static void HandleTcpClient(TcpClient client)
        {
            try
            {
                client.ReceiveTimeout = 5000;
                client.SendTimeout = 10000;
                using (client)
                using (NetworkStream stream = client.GetStream())
                using (var reader = new StreamReader(stream, Encoding.ASCII, false, 1024, true))
                {
                    string requestLine = reader.ReadLine();
                    if (string.IsNullOrEmpty(requestLine)) return;

                    // Drain headers
                    string headerLine;
                    while (!string.IsNullOrEmpty(headerLine = reader.ReadLine())) { }

                    string[] reqParts = requestLine.Split(' ');
                    if (reqParts.Length < 2) return;

                    string rawTarget = reqParts[1];
                    string path = rawTarget;
                    string queryStr = "";
                    int qIdx = rawTarget.IndexOf('?');
                    if (qIdx >= 0)
                    {
                        path = rawTarget.Substring(0, qIdx);
                        queryStr = rawTarget.Substring(qIdx + 1);
                    }
                    path = path.ToLowerInvariant();
                    var query = ParseQuery(queryStr);

                    if (path == "/status")
                    {
                        bool connected = (_camera != null && _camera.IsSessionOpen) || _captureInProgress;
                        bool lvActive = (!_captureInProgress && _camera != null && _camera.IsSessionOpen && _camera.IsInLiveViewMode);
                        string json = string.Format(
                            "{{\"connected\":{0},\"model\":\"{1}\",\"live_view\":{2},\"capturing\":{3},\"error\":\"{4}\"}}",
                            connected ? "true" : "false",
                            EscapeJson(_cameraModel),
                            lvActive ? "true" : "false",
                            _captureInProgress ? "true" : "false",
                            EscapeJson(_lastError)
                        );
                        WriteHttpBytes(stream, 200, "OK", "application/json", Encoding.UTF8.GetBytes(json));
                    }
                    else if (path == "/liveview.jpg")
                    {
                        if (_captureInProgress)
                        {
                            WriteHttpText(stream, 503, "Service Unavailable", "Capture in progress");
                            return;
                        }
                        _wantLiveView = true;
                        if (!_captureInProgress && (_camera == null || !_camera.IsSessionOpen))
                        {
                            SafeStaInvoke(delegate { if (!_captureInProgress) TryConnectCamera(); });
                        }

                        if (_captureInProgress || _camera == null || !_camera.IsSessionOpen)
                        {
                            WriteHttpText(stream, 503, "Service Unavailable", "Camera busy or not connected");
                            return;
                        }

                        if (!_captureInProgress)
                        {
                            bool needWarm = false;
                            lock (_lvLock)
                            {
                                needWarm = (_latestLvJpeg == null || (DateTime.UtcNow - _latestLvUtc).TotalSeconds > 2.0);
                            }
                            if (needWarm)
                            {
                                SafeStaInvoke(delegate
                                {
                                    try
                                    {
                                        if (_camera != null && !_captureInProgress)
                                        {
                                            if (!_camera.IsInHostLiveViewMode)
                                            {
                                                _camera.StartLiveView();
                                                if (!_camera.IsInHostLiveViewMode)
                                                    _camera.LiveViewDevice = EosLiveViewDevice.Host;
                                            }
                                            bool warmOk = _camera.DownloadEvf();
                                            Log("Warmup LV: Device=" + _camera.LiveViewDevice + " IsHost=" + _camera.IsInHostLiveViewMode + " DownloadEvf=" + warmOk);
                                        }
                                    }
                                    catch (Exception ex) { _lastError = ex.Message; Log("Warmup LV ex: " + ex.Message); }
                                });
                                for (int w = 0; w < 25; w++)
                                {
                                    lock (_lvLock)
                                    {
                                        if (_latestLvJpeg != null && (DateTime.UtcNow - _latestLvUtc).TotalSeconds < 2.0)
                                            break;
                                    }
                                    Thread.Sleep(60);
                                }
                            }
                        }

                        byte[] frame = null;
                        lock (_lvLock)
                        {
                            if (_latestLvJpeg != null && (DateTime.UtcNow - _latestLvUtc).TotalSeconds < 3.0)
                                frame = _latestLvJpeg;
                        }

                        if (frame != null && frame.Length > 0)
                        {
                            WriteHttpBytes(stream, 200, "OK", "image/jpeg", frame);
                        }
                        else
                        {
                            WriteHttpText(stream, 503, "Service Unavailable", "LiveView warming up");
                        }
                    }
                    else if (path == "/liveview/stop")
                    {
                        _wantLiveView = false;
                        SafeStaInvoke(delegate
                        {
                            try
                            {
                                if (_camera != null && _camera.IsInLiveViewMode)
                                    _camera.StopLiveView();
                            }
                            catch { }
                        });
                        WriteHttpText(stream, 200, "OK", "OK");
                    }
                    else if (path == "/capture")
                    {
                        string dest = null;
                        if (!query.TryGetValue("dest", out dest) || string.IsNullOrEmpty(dest))
                        {
                            WriteHttpText(stream, 400, "Bad Request", "Missing dest query param");
                            return;
                        }

                        lock (_capLock)
                        {
                            _captureInProgress = true;
                            _captureTargetPath = dest;
                            _captureError = null;
                            _captureDone = new ManualResetEventSlim(false);

                            // Cleanly stop LiveView & release EDSDK session so CameraControlCmd.exe snaps reliably
                            SafeStaInvoke(delegate
                            {
                                try
                                {
                                    if (_camera != null)
                                    {
                                        if (_camera.IsInLiveViewMode || _camera.IsInHostLiveViewMode)
                                        {
                                            Log("Stopping LiveView before DSLR snap...");
                                            _camera.StopLiveView();
                                            _camera.IsInLiveViewMode = false;
                                            Thread.Sleep(250);
                                        }
                                        CleanupCamera();
                                    }
                                }
                                catch (Exception ex)
                                {
                                    Log("Pre-capture cleanup WARN: " + ex.Message);
                                    CleanupCamera();
                                }
                            });

                            Thread.Sleep(200);
                            string cliErr = RunDigiCamControlCli(dest);
                            SafeStaInvoke(TryConnectCamera);
                            _captureInProgress = false;

                            if (_wantLiveView)
                            {
                                SafeStaInvoke(delegate
                                {
                                    try
                                    {
                                        if (_camera != null && !_camera.IsInHostLiveViewMode)
                                        {
                                            _camera.StartLiveView();
                                            if (!_camera.IsInHostLiveViewMode)
                                                _camera.LiveViewDevice = EosLiveViewDevice.Host;
                                        }
                                    }
                                    catch { }
                                });
                            }

                            if (!string.IsNullOrEmpty(cliErr))
                            {
                                WriteHttpText(stream, 500, "Internal Server Error", cliErr);
                            }
                            else if (File.Exists(dest))
                            {
                                WriteHttpText(stream, 200, "OK", dest);
                            }
                            else
                            {
                                WriteHttpText(stream, 500, "Internal Server Error", "Photo file was not created");
                            }
                        }
                    }
                    else if (path == "/shutdown")
                    {
                        WriteHttpText(stream, 200, "OK", "Shutting down");
                        ShutdownApp();
                    }
                    else
                    {
                        WriteHttpText(stream, 404, "Not Found", "Not Found");
                    }
                }
            }
            catch { }
        }

        private static string RunDigiCamControlCli(string dest)
        {
            string[] clis = new string[]
            {
                @"C:\Program Files (x86)\digiCamControl\CameraControlCmd.exe",
                @"C:\Program Files\digiCamControl\CameraControlCmd.exe",
                "CameraControlCmd.exe"
            };
            string cli = null;
            foreach (string c in clis)
            {
                if (File.Exists(c)) { cli = c; break; }
            }
            if (cli == null) return "CameraControlCmd.exe not found";

            try
            {
                string parent = Path.GetDirectoryName(dest);
                if (!string.IsNullOrEmpty(parent)) Directory.CreateDirectory(parent);
                if (File.Exists(dest)) File.Delete(dest);

                var psi = new System.Diagnostics.ProcessStartInfo();
                psi.FileName = cli;
                psi.Arguments = "/filename \"" + dest.Replace('/', '\\') + "\" /capture";
                psi.UseShellExecute = false;
                psi.CreateNoWindow = true;

                Log("Running CLI shutter: " + psi.Arguments);
                using (var proc = System.Diagnostics.Process.Start(psi))
                {
                    if (!proc.WaitForExit(14000))
                    {
                        try { proc.Kill(); } catch { }
                        return "CameraControlCmd.exe timed out";
                    }
                }
                if (File.Exists(dest) && new FileInfo(dest).Length > 10000)
                {
                    Log("CLI captured DSLR photo (" + new FileInfo(dest).Length + " bytes) to " + dest);
                    return null;
                }
                return "CLI finished without saving photo";
            }
            catch (Exception ex)
            {
                return ex.Message;
            }
        }

        private static void ShutdownApp()
        {
            _running = false;
            try { if (_listener != null) _listener.Stop(); } catch { }
            SafeStaInvoke(delegate
            {
                try { if (_camera != null && _camera.IsInLiveViewMode) _camera.StopLiveView(); } catch { }
                CleanupCamera();
                try { if (_framework != null) _framework.Dispose(); } catch { }
                Application.Exit();
            });
        }

        private static void WriteHttpText(NetworkStream stream, int code, string reason, string text)
        {
            WriteHttpBytes(stream, code, reason, "text/plain; charset=utf-8", Encoding.UTF8.GetBytes(text ?? ""));
        }

        private static void WriteHttpBytes(NetworkStream stream, int code, string reason, string contentType, byte[] body)
        {
            string headers = string.Format(
                "HTTP/1.1 {0} {1}\r\nContent-Type: {2}\r\nContent-Length: {3}\r\nConnection: close\r\n\r\n",
                code, reason, contentType, body.Length
            );
            byte[] headerBytes = Encoding.ASCII.GetBytes(headers);
            stream.Write(headerBytes, 0, headerBytes.Length);
            if (body.Length > 0)
            {
                stream.Write(body, 0, body.Length);
            }
            stream.Flush();
        }

        private static string EscapeJson(string s)
        {
            if (string.IsNullOrEmpty(s)) return "";
            return s.Replace("\\", "\\\\").Replace("\"", "\\\"").Replace("\r", "").Replace("\n", " ");
        }
    }
}
