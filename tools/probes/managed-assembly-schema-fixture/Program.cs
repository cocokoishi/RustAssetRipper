using System;
using System.Collections.Generic;

namespace UnityEngine
{
    public class Object { }
    public class MonoBehaviour : Object { }

    [AttributeUsage(AttributeTargets.Field)]
    public sealed class SerializeField : Attribute { }
}

namespace Game
{
    [Serializable]
    public struct NestedStats
    {
        public int score;
        public string label;
    }

    public abstract class BaseController : UnityEngine.MonoBehaviour
    {
        public long sequence;
    }

    public enum Phase : byte
    {
        Idle,
        Active,
    }

    public sealed class RichController : BaseController
    {
        public int health;
        public int[] samples = Array.Empty<int>();
        public List<float> weights = new();
        public NestedStats stats;
        public UnityEngine.Object? target;

        [UnityEngine.SerializeField]
        private bool active = true;

        [NonSerialized]
        public int ignored;

        public Phase phase;
        public static int staticValue;
        public readonly int locked;

        public bool IsActive => active;
    }
}
