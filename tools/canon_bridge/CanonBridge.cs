using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Windows.Forms;
using CameraControl.Devices;
using CameraControl.Devices.Canon;
using CameraControl.Devices.Classes;
using Canon.Eos.Framework;

namespace CanonBridge
{
    internal static class Program
    {
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool SetDllDirectory(string lpPathName);

        private static readonly object _lvLock = new object();
        private static byte[] _latestLvJpeg = null;
        private static DateTime _latestLvUtc = DateTime.MinValue;
        private static volatile bool _lvStarted = false;

        private static readonly object _capLock = new object();
        private static ManualResetEventSlim _captureDone = null;
        private static string _captureTargetPath = null;
        private static string _captureError = null;
        private static volatile bool _captureInProgress = false;

        private static Control _staInvoker = null;
        private static EosFramework _framework = null;
        private static CanonSDKBase _sdk = null;
        private static string _cameraModel = "";
        private static string _lastError = "";
        private static volatile bool _wantLiveView = false;

        private static TcpListener _listener = null;
        private static volatile bool _running = true;

        private static bool _lvDiagLogged = false;
        private static int _lvErrorCount = 0;

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

        [STAThread]
        private static void Main(string[] args)
        {
            ConfigureNativeAndAssemblySearchPaths();

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

            Log("Starting CanonBridge (CanonSDKBase in-session) on port " + port + " (parentPid=" + parentPid + ")");

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

            // Reconnect watchdog timer on STA thread (1500ms) - never touch camera while capturing
            var timer = new System.Windows.Forms.Timer();
            timer.Interval = 1500;
            timer.Tick += delegate
            {
                if (_captureInProgress) return;
                if (_sdk == null || !_sdk.IsConnected)
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
                if (_wantLiveView && !_captureInProgress && _sdk != null && _sdk.IsConnected)
                {
                    PollLiveViewOnce();
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
                        var proc = Process.GetProcessById(parentPid);
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

        private static void PollLiveViewOnce()
        {
            try
            {
                if (_sdk == null || !_sdk.IsConnected || _captureInProgress) return;

                if (!_lvStarted)
                {
                    _sdk.StartLiveView();
                    _lvStarted = true;
                }

                LiveViewData lv = _sdk.GetLiveViewImage();
                if (lv != null && lv.ImageData != null && lv.ImageData.Length > 100)
                {
                    bool firstFrame = false;
                    lock (_lvLock)
                    {
                        if (_latestLvJpeg == null) firstFrame = true;
                        _latestLvJpeg = lv.ImageData;
                        _latestLvUtc = DateTime.UtcNow;
                    }
                    if (!_lvDiagLogged || firstFrame)
                    {
                        _lvDiagLogged = true;
                        Log("LiveView EVF frame OK (" + lv.ImageData.Length + " bytes)");
                    }
                }
            }
            catch (Exception ex)
            {
                _lvStarted = false;
                _lvErrorCount++;
                if (_lvErrorCount <= 5)
                {
                    Log("LV Poll exception #" + _lvErrorCount + ": " + ex.Message);
                }
            }
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
                if (_sdk != null)
                {
                    if (_sdk.IsConnected && _sdk.Camera != null && _sdk.Camera.IsSessionOpen) return;
                    CleanupCamera();
                }

                EosCamera cam = _framework.GetCamera();
                if (cam == null)
                {
                    _cameraModel = "";
                    return;
                }

                var sdk = new CanonSDKBase();
                bool ok = sdk.Init(cam);
                if (!ok)
                {
                    try { sdk.Close(); } catch { }
                    try { cam.Dispose(); } catch { }
                    _lastError = "CanonSDKBase.Init returned false";
                    Log(_lastError);
                    return;
                }

                try { sdk.CaptureInSdRam = true; }
                catch (Exception sdEx) { Log("CaptureInSdRam WARN: " + sdEx.Message); }

                sdk.PhotoCaptured += OnPhotoCaptured;
                sdk.CameraDisconnected += delegate { OnCameraDisconnected(); };

                _sdk = sdk;
                _lvStarted = false;
                _lvDiagLogged = false;
                _lvErrorCount = 0;
                _cameraModel = !string.IsNullOrEmpty(sdk.DeviceName)
                    ? sdk.DeviceName
                    : (cam.DeviceDescription ?? "Canon EOS");
                _lastError = "";
                Log("Connected camera via CanonSDKBase: " + _cameraModel);

                if (_wantLiveView)
                {
                    try
                    {
                        _sdk.StartLiveView();
                        _lvStarted = true;
                    }
                    catch (Exception ex)
                    {
                        _lastError = ex.Message;
                    }
                }
            }
            catch (Exception ex)
            {
                _lastError = ex.Message;
                Log("TryConnectCamera error: " + ex.Message);
                CleanupCamera();
            }
        }

        private static void CleanupCamera()
        {
            _lvStarted = false;
            if (_sdk != null)
            {
                try { _sdk.PhotoCaptured -= OnPhotoCaptured; } catch { }
                try { _sdk.Close(); } catch { }
                if (_sdk.Camera != null)
                {
                    try { _sdk.Camera.Dispose(); } catch { }
                }
                _sdk = null;
            }
            _cameraModel = "";
            lock (_lvLock)
            {
                _latestLvJpeg = null;
            }
        }

        private static void OnPhotoCaptured(object sender, PhotoCapturedEventArgs e)
        {
            // Transfer on a worker thread so the EDSDK / WinForms STA pump stays responsive
            ThreadPool.QueueUserWorkItem(delegate
            {
                var sw = Stopwatch.StartNew();
                try
                {
                    string dest = _captureTargetPath;
                    Log("PhotoCaptured event fired: FileName=" + (e != null ? e.FileName : ""));
                    if (!string.IsNullOrEmpty(dest) && e != null && e.CameraDevice != null)
                    {
                        string parent = Path.GetDirectoryName(dest);
                        if (!string.IsNullOrEmpty(parent)) Directory.CreateDirectory(parent);

                        e.CameraDevice.TransferFile(e.Handle, dest);
                        long bytes = File.Exists(dest) ? new FileInfo(dest).Length : 0;
                        Log("Transferred DSLR photo (" + bytes + " bytes in " + sw.ElapsedMilliseconds + "ms) -> " + dest);
                    }
                    _captureError = null;
                }
                catch (Exception ex)
                {
                    _captureError = ex.Message;
                    Log("OnPhotoCaptured transfer error: " + ex.Message);
                }
                finally
                {
                    var evt = _captureDone;
                    if (evt != null) evt.Set();
                }
            });
        }

        private static void OnCameraDisconnected()
        {
            Log("CameraDisconnected event received");
            SafeStaInvoke(CleanupCamera);
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
                        bool connected = (_sdk != null && _sdk.IsConnected) || _captureInProgress;
                        bool lvActive = (!_captureInProgress && _sdk != null && _sdk.IsConnected && _lvStarted);
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
                        if (!_captureInProgress && (_sdk == null || !_sdk.IsConnected))
                        {
                            SafeStaInvoke(delegate { if (!_captureInProgress) TryConnectCamera(); });
                        }

                        if (_captureInProgress || _sdk == null || !_sdk.IsConnected)
                        {
                            WriteHttpText(stream, 503, "Service Unavailable", "Camera busy or not connected");
                            return;
                        }

                        bool needWarm = false;
                        lock (_lvLock)
                        {
                            needWarm = (_latestLvJpeg == null || (DateTime.UtcNow - _latestLvUtc).TotalSeconds > 2.0);
                        }
                        if (needWarm && !_captureInProgress)
                        {
                            SafeStaInvoke(delegate { PollLiveViewOnce(); });
                            for (int w = 0; w < 20; w++)
                            {
                                if (_captureInProgress) break;
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
                                if (_sdk != null && _lvStarted)
                                {
                                    _sdk.StopLiveView();
                                    _lvStarted = false;
                                }
                            }
                            catch { _lvStarted = false; }
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
                            var capSw = Stopwatch.StartNew();
                            _captureInProgress = true;
                            _captureTargetPath = dest;
                            _captureError = null;
                            _captureDone = new ManualResetEventSlim(false);

                            try
                            {
                                string parent = Path.GetDirectoryName(dest);
                                if (!string.IsNullOrEmpty(parent)) Directory.CreateDirectory(parent);
                                if (File.Exists(dest)) File.Delete(dest);
                            }
                            catch { }

                            bool sdkReady = false;
                            SafeStaInvoke(delegate
                            {
                                if (_sdk == null || !_sdk.IsConnected)
                                {
                                    TryConnectCamera();
                                }
                                if (_sdk != null && _sdk.IsConnected)
                                {
                                    sdkReady = true;
                                    try
                                    {
                                        if (_lvStarted)
                                        {
                                            _sdk.StopLiveView();
                                            _lvStarted = false;
                                        }
                                    }
                                    catch (Exception ex)
                                    {
                                        _lvStarted = false;
                                        Log("StopLiveView pre-capture WARN: " + ex.Message);
                                    }
                                }
                            });

                            bool captured = false;
                            string captureErr = null;

                            if (sdkReady)
                            {
                                Thread.Sleep(100);
                                bool afTriggered = false;
                                SafeStaInvoke(delegate
                                {
                                    try
                                    {
                                        if (_sdk != null && _sdk.IsConnected)
                                        {
                                            Log("Triggering in-session CapturePhoto() [StopLV at " + capSw.ElapsedMilliseconds + "ms]");
                                            _sdk.CapturePhoto();
                                            afTriggered = true;
                                        }
                                    }
                                    catch (Exception ex)
                                    {
                                        Log("CapturePhoto() threw (" + ex.Message + "), falling back to CapturePhotoNoAf()");
                                        try
                                        {
                                            if (_sdk != null && _sdk.IsConnected)
                                            {
                                                _sdk.CapturePhotoNoAf();
                                                afTriggered = true;
                                            }
                                        }
                                        catch (Exception noAfEx)
                                        {
                                            captureErr = noAfEx.Message;
                                            Log("CapturePhotoNoAf() threw: " + noAfEx.Message);
                                        }
                                    }
                                });

                                if (afTriggered)
                                {
                                    captured = _captureDone.Wait(4800);
                                    if (!captured)
                                    {
                                        Log("CapturePhoto did not complete within 4800ms, retrying CapturePhotoNoAf()");
                                        SafeStaInvoke(delegate
                                        {
                                            try
                                            {
                                                if (_sdk != null && _sdk.IsConnected)
                                                    _sdk.CapturePhotoNoAf();
                                            }
                                            catch (Exception ex2)
                                            {
                                                captureErr = ex2.Message;
                                                Log("Retry CapturePhotoNoAf() threw: " + ex2.Message);
                                            }
                                        });
                                        captured = _captureDone.Wait(6500);
                                    }
                                }
                            }

                            if (!captured || !File.Exists(dest) || new FileInfo(dest).Length <= 10000)
                            {
                                Log("In-session capture incomplete (captured=" + captured + ", err=" + (_captureError ?? captureErr ?? "none") + "), falling back to CameraControlCmd.exe");
                                SafeStaInvoke(CleanupCamera);
                                Thread.Sleep(150);
                                string cliErr = RunDigiCamControlCli(dest);
                                SafeStaInvoke(TryConnectCamera);
                                captureErr = cliErr;
                            }
                            else
                            {
                                captureErr = _captureError;
                            }

                            _captureInProgress = false;

                            if (_wantLiveView)
                            {
                                Thread.Sleep(80);
                                SafeStaInvoke(delegate
                                {
                                    try
                                    {
                                        if (_sdk != null && _sdk.IsConnected && !_lvStarted)
                                        {
                                            _sdk.StartLiveView();
                                            _lvStarted = true;
                                        }
                                    }
                                    catch { _lvStarted = false; }
                                });
                            }

                            Log("Total /capture finished in " + capSw.ElapsedMilliseconds + "ms -> " + (File.Exists(dest) ? "OK" : "FAILED"));

                            if (!string.IsNullOrEmpty(captureErr))
                            {
                                WriteHttpText(stream, 500, "Internal Server Error", captureErr);
                            }
                            else if (File.Exists(dest) && new FileInfo(dest).Length > 10000)
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

                var psi = new ProcessStartInfo();
                psi.FileName = cli;
                psi.Arguments = "/filename \"" + dest.Replace('/', '\\') + "\" /capture";
                psi.UseShellExecute = false;
                psi.CreateNoWindow = true;

                Log("Running CLI fallback shutter: " + psi.Arguments);
                using (var proc = Process.Start(psi))
                {
                    if (!proc.WaitForExit(14000))
                    {
                        try { proc.Kill(); } catch { }
                        return "CameraControlCmd.exe timed out";
                    }
                }
                if (File.Exists(dest) && new FileInfo(dest).Length > 10000)
                {
                    Log("CLI fallback captured DSLR photo (" + new FileInfo(dest).Length + " bytes) to " + dest);
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
                try { if (_sdk != null && _lvStarted) _sdk.StopLiveView(); } catch { }
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
