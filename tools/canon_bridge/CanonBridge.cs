using System;
using System.Collections.Generic;
using System.IO;
using System.Net;
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

        private static HttpListener _listener = null;
        private static volatile bool _running = true;

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

            // Invisible Win32 message-pump Control on the STA thread
            _staInvoker = new Control();
            IntPtr _forceHandle = _staInvoker.Handle;

            try
            {
                _framework = new EosFramework();
                _framework.CameraAdded += delegate { SafeStaInvoke(TryConnectCamera); };
                TryConnectCamera();
            }
            catch (Exception ex)
            {
                _lastError = "Framework init error: " + ex.Message;
            }

            // Reconnect / LiveView watchdog timer on STA thread
            var timer = new System.Windows.Forms.Timer();
            timer.Interval = 1500;
            timer.Tick += delegate
            {
                if (_camera == null || !_camera.IsSessionOpen)
                {
                    TryConnectCamera();
                }
                else if (_wantLiveView && !_captureInProgress && !_camera.IsInLiveViewMode)
                {
                    try { _camera.StartLiveView(); } catch { }
                }
            };
            timer.Start();

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

            // Start HTTP listener thread
            var httpThread = new Thread(delegate() { RunHttpServer(port); });
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

                cam.IsErrorTolerantMode = true;
                cam.EnsureOpenSession();
                cam.SavePicturesToHost(_tempSaveDir);
                cam.LiveViewUpdate += OnLiveViewUpdate;
                cam.PictureTaken += OnPictureTaken;
                cam.Error += OnCameraError;
                cam.Shutdown += OnCameraShutdown;

                _camera = cam;
                _cameraModel = cam.DeviceDescription ?? "Canon EOS";
                _lastError = "";

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
                    lock (_lvLock)
                    {
                        _latestLvJpeg = data;
                        _latestLvUtc = DateTime.UtcNow;
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
                }
                _captureError = null;
            }
            catch (Exception ex)
            {
                _captureError = ex.Message;
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
            }
        }

        private static void OnCameraShutdown(object sender, EventArgs e)
        {
            CleanupCamera();
        }

        private static void RunHttpServer(int port)
        {
            try
            {
                _listener = new HttpListener();
                _listener.Prefixes.Add("http://127.0.0.1:" + port + "/");
                _listener.Start();
            }
            catch (Exception ex)
            {
                _lastError = "HttpListener start error: " + ex.Message;
                return;
            }

            while (_running)
            {
                try
                {
                    HttpListenerContext ctx = _listener.GetContext();
                    ThreadPool.QueueUserWorkItem(delegate(object state)
                    {
                        HandleRequest((HttpListenerContext)state);
                    }, ctx);
                }
                catch
                {
                    if (!_running) break;
                }
            }
        }

        private static void HandleRequest(HttpListenerContext ctx)
        {
            try
            {
                string path = ctx.Request.Url.AbsolutePath.ToLowerInvariant();
                if (path == "/status")
                {
                    bool connected = (_camera != null && _camera.IsSessionOpen);
                    bool lvActive = (connected && _camera.IsInLiveViewMode);
                    string json = string.Format(
                        "{{\"connected\":{0},\"model\":\"{1}\",\"live_view\":{2},\"error\":\"{3}\"}}",
                        connected ? "true" : "false",
                        EscapeJson(_cameraModel),
                        lvActive ? "true" : "false",
                        EscapeJson(_lastError)
                    );
                    WriteText(ctx.Response, 200, "application/json", json);
                }
                else if (path == "/liveview.jpg")
                {
                    _wantLiveView = true;
                    if (_camera == null || !_camera.IsSessionOpen)
                    {
                        SafeStaInvoke(TryConnectCamera);
                    }

                    if (_camera == null || !_camera.IsSessionOpen)
                    {
                        WriteText(ctx.Response, 503, "text/plain", "Camera not connected");
                        return;
                    }

                    if (!_camera.IsInLiveViewMode && !_captureInProgress)
                    {
                        SafeStaInvoke(delegate
                        {
                            try
                            {
                                if (_camera != null && !_camera.IsInLiveViewMode && !_captureInProgress)
                                    _camera.StartLiveView();
                            }
                            catch (Exception ex) { _lastError = ex.Message; }
                        });
                        // Wait briefly for first frame
                        for (int w = 0; w < 15; w++)
                        {
                            lock (_lvLock)
                            {
                                if (_latestLvJpeg != null && (DateTime.UtcNow - _latestLvUtc).TotalSeconds < 2.0)
                                    break;
                            }
                            Thread.Sleep(50);
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
                        ctx.Response.StatusCode = 200;
                        ctx.Response.ContentType = "image/jpeg";
                        ctx.Response.ContentLength64 = frame.Length;
                        ctx.Response.OutputStream.Write(frame, 0, frame.Length);
                        ctx.Response.OutputStream.Close();
                    }
                    else
                    {
                        WriteText(ctx.Response, 503, "text/plain", "LiveView warming up");
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
                    WriteText(ctx.Response, 200, "text/plain", "OK");
                }
                else if (path == "/capture")
                {
                    string dest = ctx.Request.QueryString["dest"];
                    if (string.IsNullOrEmpty(dest))
                    {
                        WriteText(ctx.Response, 400, "text/plain", "Missing dest query param");
                        return;
                    }

                    lock (_capLock)
                    {
                        if (_camera == null || !_camera.IsSessionOpen)
                        {
                            SafeStaInvoke(TryConnectCamera);
                        }
                        if (_camera == null || !_camera.IsSessionOpen)
                        {
                            WriteText(ctx.Response, 503, "text/plain", "Camera not connected");
                            return;
                        }

                        _captureInProgress = true;
                        _captureTargetPath = dest;
                        _captureError = null;
                        _captureDone = new ManualResetEventSlim(false);

                        string triggerErr = null;
                        SafeStaInvoke(delegate
                        {
                            try
                            {
                                _camera.SavePicturesToHost(_tempSaveDir);
                                if (_camera.IsInLiveViewMode)
                                {
                                    try
                                    {
                                        _camera.TakePictureInLiveview();
                                    }
                                    catch
                                    {
                                        _camera.PauseLiveview();
                                        _camera.TakePictureNoAf();
                                    }
                                }
                                else
                                {
                                    try { _camera.TakePicture(); }
                                    catch { _camera.TakePictureNoAf(); }
                                }
                            }
                            catch (Exception ex)
                            {
                                triggerErr = ex.Message;
                                _captureDone.Set();
                            }
                        });

                        bool signaled = _captureDone.Wait(15000);
                        _captureInProgress = false;

                        // Resume LiveView for next pose if requested
                        if (_wantLiveView)
                        {
                            SafeStaInvoke(delegate
                            {
                                try
                                {
                                    if (_camera != null && !_camera.IsInLiveViewMode)
                                        _camera.StartLiveView();
                                    else if (_camera != null)
                                        _camera.ResumeLiveview();
                                }
                                catch { }
                            });
                        }

                        if (!signaled)
                        {
                            WriteText(ctx.Response, 504, "text/plain", "Capture timeout waiting for DSLR");
                        }
                        else if (!string.IsNullOrEmpty(triggerErr))
                        {
                            WriteText(ctx.Response, 500, "text/plain", triggerErr);
                        }
                        else if (!string.IsNullOrEmpty(_captureError))
                        {
                            WriteText(ctx.Response, 500, "text/plain", _captureError);
                        }
                        else if (File.Exists(dest))
                        {
                            WriteText(ctx.Response, 200, "text/plain", dest);
                        }
                        else
                        {
                            WriteText(ctx.Response, 500, "text/plain", "Photo file was not created");
                        }
                    }
                }
                else if (path == "/shutdown")
                {
                    WriteText(ctx.Response, 200, "text/plain", "Shutting down");
                    ShutdownApp();
                }
                else
                {
                    WriteText(ctx.Response, 404, "text/plain", "Not Found");
                }
            }
            catch (Exception ex)
            {
                try { WriteText(ctx.Response, 500, "text/plain", ex.Message); } catch { }
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

        private static void WriteText(HttpListenerResponse resp, int status, string contentType, string text)
        {
            byte[] buf = Encoding.UTF8.GetBytes(text ?? "");
            resp.StatusCode = status;
            resp.ContentType = contentType;
            resp.ContentLength64 = buf.Length;
            resp.OutputStream.Write(buf, 0, buf.Length);
            resp.OutputStream.Close();
        }

        private static string EscapeJson(string s)
        {
            if (string.IsNullOrEmpty(s)) return "";
            return s.Replace("\\", "\\\\").Replace("\"", "\\\"").Replace("\r", "").Replace("\n", " ");
        }
    }
}
